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
    external: Option<&'a (dyn Fn() -> bool + Sync)>,
}
impl<'a> Cancel<'a> {
    pub fn new(stop: &'a AtomicBool) -> Self {
        Self {
            stop,
            revision: None,
            external: None,
        }
    }
    pub fn for_revision(stop: &'a AtomicBool, revision: &'a AtomicU64, expected: u64) -> Self {
        Self {
            stop,
            revision: Some((revision, expected)),
            external: None,
        }
    }
    /// Preserve original native-request cancellation through bounded decoders
    /// without a polling thread or replacing the worker's own lifetime flag.
    pub fn with_external_cancellation(mut self, cancelled: &'a (dyn Fn() -> bool + Sync)) -> Self {
        self.external = Some(cancelled);
        self
    }
    pub fn is_cancelled(self) -> bool {
        self.stop.load(Ordering::Acquire)
            || self.external.is_some_and(|cancelled| cancelled())
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
    // Last: every logical reservation keeps physical storage admitted through
    // the last retained model, texture, source map or emitted-frame owner.
    _storage: Option<Arc<ilium_execution::StorageAdmission>>,
}

/// Clone this same budget into workers and retained snapshots. Creating a new
/// independent budget per request defeats the process-level limit; D2c owns one
/// shared budget for this scene family.
#[derive(Debug, Clone)]
pub struct ByteBudget(Arc<Counter>);
impl ByteBudget {
    pub fn new(limit: u64) -> Result<Self> {
        Self::new_with_storage(limit, None)
    }
    /// The caller must reserve this complete limit before creating payloads.
    /// Clones cover the same allocations; they never invent a second quota.
    pub(crate) fn with_storage(
        limit: u64,
        storage: Arc<ilium_execution::StorageAdmission>,
    ) -> Result<Self> {
        Self::new_with_storage(limit, Some(storage))
    }
    fn new_with_storage(
        limit: u64,
        storage: Option<Arc<ilium_execution::StorageAdmission>>,
    ) -> Result<Self> {
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
            _storage: storage,
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
    /// Release unused admission headroom after the retained payload is measured.
    /// The existing owner keeps the same account; growth requires fresh admission.
    pub(crate) fn shrink_to(&mut self, bytes: u64) -> Result<()> {
        let released = self.bytes.checked_sub(bytes).ok_or(AssetError::Limit {
            resource: "reservation shrink",
            requested: bytes,
            limit: self.bytes,
        })?;
        self.counter.used.fetch_sub(released, Ordering::AcqRel);
        self.bytes = bytes;
        Ok(())
    }
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
    fn physical_storage_survives_budget_handle_until_last_retained_reservation() {
        use ilium_execution::{QuotaGroup, QuotaLimits};
        let quota = QuotaGroup::new(QuotaLimits {
            clients: 0,
            jobs: 0,
            service_jobs: 0,
            input_bytes: 0,
            result_bytes: 0,
            worker_threads: 0,
            worker_bytes: 16,
        });
        let physical = Arc::new(quota.reserve_external_storage(16).unwrap());
        let budget = ByteBudget::with_storage(16, physical).unwrap();
        let stop = AtomicBool::new(false);
        let owner = budget.reserve(6, Cancel::new(&stop)).unwrap();
        let second_owner = budget.clone().reserve(4, Cancel::new(&stop)).unwrap();
        drop(budget);
        assert_eq!(quota.snapshot().worker_bytes, 16);
        assert_eq!(quota.snapshot().worker_threads, 0);
        assert!(quota.reserve_external_storage(1).is_err());
        drop(owner);
        assert_eq!(quota.snapshot().worker_bytes, 16);
        drop(second_owner);
        assert_eq!(quota.snapshot().worker_bytes, 0);
        let replacement = quota.reserve_external_storage(16).unwrap();
        drop(replacement);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }

    #[test]
    fn shrink_releases_only_unused_headroom_and_rejects_growth_without_mutation() {
        let stop = AtomicBool::new(false);
        let cancel = Cancel::new(&stop);
        let budget = ByteBudget::new(16).unwrap();
        let mut original = budget.reserve(16, cancel).unwrap();
        original.shrink_to(6).unwrap();
        assert_eq!(budget.used(), 6);
        assert_eq!(original.bytes(), 6);
        assert!(original.shrink_to(7).is_err());
        assert_eq!(budget.used(), 6);
        let concurrent = budget.clone().reserve(10, cancel).unwrap();
        original.shrink_to(6).unwrap();
        assert_eq!(budget.used(), 16);
        original.shrink_to(0).unwrap();
        assert_eq!(budget.used(), 10);
        drop(original);
        assert_eq!(budget.used(), 10);
        drop(concurrent);
        assert_eq!(budget.used(), 0);
        assert_eq!(budget.peak(), 16);
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
    fn original_external_cancellation_refuses_new_budget_work_without_releasing_retained_credit() {
        let native = AtomicBool::new(false);
        let external = ilium_platform::owned_worker::StopToken::default();
        let child = external.child();
        let cancelled = || child.is_stopped();
        let cancel = Cancel::new(&native).with_external_cancellation(&cancelled);
        let budget = ByteBudget::new(32).unwrap();
        let retained = budget.reserve(16, cancel).unwrap();
        external.stop();
        assert!(!native.load(Ordering::Acquire));
        assert_eq!(cancel.check(), Err(AssetError::Cancelled));
        assert!(budget.reserve(1, cancel).is_err());
        assert_eq!(budget.used(), 16);
        drop(retained);
        assert_eq!(budget.used(), 0);
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
