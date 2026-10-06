//! Copy an admitted native region into an independently owned V8 completion.
//! Source identity is metadata; only the original admission establishes custody.
use crate::{
    engine::{ArraySpec, EngineLimits, ServiceValue, TypedArrayKind},
    error::{AnimationError, Result},
    world_region_encoding::EncodedRegion,
};
use ilium_execution::{QuotaGroup, StorageAdmission};
use ilium_platform::owned_worker::StopToken;
use serde_json::Value;
use std::collections::BTreeMap;

enum NativeRegionPlane<'a> {
    Borrowed(&'a [u8]),
    Converted {
        bytes: Vec<u8>,
        _admission: StorageAdmission,
    },
}
impl NativeRegionPlane<'_> {
    fn as_slice(&self) -> &[u8] {
        match self {
            Self::Borrowed(bytes) => bytes,
            Self::Converted { bytes, .. } => bytes,
        }
    }
}
#[cfg(test)]
fn native_region_plane<'a>(
    wire: &'a [u8],
    quota: &QuotaGroup,
    big_endian: bool,
) -> Result<NativeRegionPlane<'a>> {
    native_region_plane_with_cancel(wire, quota, big_endian, || false)
}

fn native_region_plane_with_cancel<'a>(
    wire: &'a [u8],
    quota: &QuotaGroup,
    big_endian: bool,
    mut cancelled: impl FnMut() -> bool,
) -> Result<NativeRegionPlane<'a>> {
    if !wire.len().is_multiple_of(2) {
        return Err(AnimationError::Runtime(
            "incomplete native region index".into(),
        ));
    }
    if cancelled() {
        return Err(AnimationError::Runtime(
            "native region completion cancelled".into(),
        ));
    }
    if !big_endian || wire.is_empty() {
        return Ok(NativeRegionPlane::Borrowed(wire));
    }
    // The encoded wire is always little endian; V8's U16 storage is native
    // endian. Admit the temporary conversion under the original root before
    // allocating, and retain it through the separately admitted result copy.
    let admission = quota
        .reserve_external_storage(wire.len())
        .map_err(|error| {
            AnimationError::Budget(format!("native region byte-order admission: {error:?}"))
        })?;
    if cancelled() {
        return Err(AnimationError::Runtime(
            "native region completion cancelled".into(),
        ));
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(wire.len())
        .map_err(|_| AnimationError::Budget("native region byte-order allocation".into()))?;
    for pair in wire.chunks_exact(2) {
        if cancelled() {
            return Err(AnimationError::Runtime(
                "native region completion cancelled".into(),
            ));
        }
        bytes.extend_from_slice(&[pair[1], pair[0]]);
    }
    if cancelled() {
        return Err(AnimationError::Runtime(
            "native region completion cancelled".into(),
        ));
    }
    Ok(NativeRegionPlane::Converted {
        bytes,
        _admission: admission,
    })
}

pub fn copy_region_response(
    encoded: &EncodedRegion<StorageAdmission>,
    limits: &EngineLimits,
    quota: QuotaGroup,
) -> Result<ServiceValue> {
    copy_region_response_with_stop(encoded, limits, quota, &StopToken::default())
}

pub(crate) fn copy_region_response_with_stop(
    encoded: &EncodedRegion<StorageAdmission>,
    limits: &EngineLimits,
    quota: QuotaGroup,
    request_stop: &StopToken,
) -> Result<ServiceValue> {
    copy_region_response_with_destination(
        encoded,
        limits,
        quota,
        request_stop,
        |metadata, arrays, plane, limits, quota| {
            ServiceValue::copy_from_borrowed_host(
                metadata,
                arrays,
                &BTreeMap::from([("b0".to_owned(), plane)]),
                limits,
                quota,
            )
        },
    )
}

#[cfg(test)]
fn copy_region_response_with_destination_hooks_for_test(
    encoded: &EncodedRegion<StorageAdmission>,
    limits: &EngineLimits,
    quota: QuotaGroup,
    request_stop: &StopToken,
    after_destination_admission: impl FnOnce(&StorageAdmission) -> Result<()>,
    after_destination_copy: impl FnOnce(&ServiceValue),
) -> Result<ServiceValue> {
    copy_region_response_with_destination(
        encoded,
        limits,
        quota,
        request_stop,
        move |metadata, arrays, plane, limits, quota| {
            let result = ServiceValue::copy_from_borrowed_host_with_admission_hook(
                metadata,
                arrays,
                &BTreeMap::from([("b0".to_owned(), plane)]),
                limits,
                quota,
                after_destination_admission,
            )?;
            after_destination_copy(&result);
            Ok(result)
        },
    )
}

fn copy_region_response_with_destination<F>(
    encoded: &EncodedRegion<StorageAdmission>,
    limits: &EngineLimits,
    quota: QuotaGroup,
    request_stop: &StopToken,
    destination_copy: F,
) -> Result<ServiceValue>
where
    F: FnOnce(&Value, &[ArraySpec], &[u8], &EngineLimits, QuotaGroup) -> Result<ServiceValue>,
{
    // Reject foreign custody before parsing or debiting the receiving bank.
    if !encoded.shares_root(&quota) {
        return Err(AnimationError::PermissionDenied(
            "foreign native region quota".into(),
        ));
    }
    if request_stop.is_stopped() {
        return Err(AnimationError::Runtime(
            "native region completion cancelled".into(),
        ));
    }
    if encoded.metadata().len() > limits.json_bytes
        || encoded.blocks().len() > limits.backing_bytes
        || !encoded.blocks().len().is_multiple_of(2)
    {
        return Err(AnimationError::Budget(
            "native region receiving limits".into(),
        ));
    }
    // The encoded shape is flat, with bounded palette strings and properties.
    // Charge its parsed tree and bridge temporaries before allocating either.
    let parsing_bytes = encoded
        .metadata()
        .len()
        .checked_mul(32)
        .and_then(|bytes| bytes.checked_add(4096))
        .ok_or_else(|| AnimationError::Budget("native region parse size".into()))?;
    let _parsing = quota
        .reserve_external_storage(parsing_bytes)
        .map_err(|error| {
            AnimationError::Budget(format!("native region parse admission: {error:?}"))
        })?;
    if request_stop.is_stopped() {
        return Err(AnimationError::Runtime(
            "native region completion cancelled".into(),
        ));
    }
    let mut metadata: Value = serde_json::from_slice(encoded.metadata())?;
    let reference = metadata
        .pointer_mut("/value/blocks/$ilium_binary")
        .filter(|reference| reference.as_str() == Some("blocks"))
        .ok_or_else(|| AnimationError::Runtime("invalid native region plane reference".into()))?;
    *reference = Value::String("b0".into());
    let arrays = [ArraySpec {
        name: "b0".into(),
        kind: TypedArrayKind::U16,
        elements: encoded.blocks().len() / 2,
    }];
    let plane = native_region_plane_with_cancel(
        encoded.blocks(),
        &quota,
        cfg!(target_endian = "big"),
        || request_stop.is_stopped(),
    )?;
    let result = destination_copy(&metadata, &arrays, plane.as_slice(), limits, quota)?;
    if request_stop.is_stopped() {
        return Err(AnimationError::Runtime(
            "native region completion cancelled".into(),
        ));
    }

    Ok(result)
}

#[cfg(test)]
mod region_plane_tests {
    use super::*;
    use crate::world_region::{
        collect_region, BlockStateView, RegionError, RegionLimits, RegionSpec,
    };
    use crate::world_region_encoding::{encode_region, EncodedRegion};
    use ilium_execution::QuotaLimits;
    use std::cell::Cell;
    fn quota(bytes: usize) -> QuotaGroup {
        QuotaGroup::new(QuotaLimits {
            clients: 1,
            jobs: 1,
            service_jobs: 0,
            input_bytes: 1,
            result_bytes: 1,
            worker_threads: 1,
            worker_bytes: bytes,
        })
    }
    #[test]
    fn little_endian_plane_borrows_exact_source_without_additional_charge() {
        let quota = quota(64);
        let wire = [0, 0, 1, 0, 0x34, 0x12, 0xff, 0xff];
        let plane = native_region_plane(&wire, &quota, false).unwrap();
        assert_eq!(plane.as_slice().as_ptr(), wire.as_ptr());
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }
    #[test]
    fn big_endian_plane_preserves_indices_and_retains_original_quota_until_drop() {
        let quota = quota(64);
        let wire = [0, 0, 1, 0, 0x34, 0x12, 0xff, 0xff];
        let plane = native_region_plane(&wire, &quota, true).unwrap();
        let values: Vec<_> = plane
            .as_slice()
            .chunks_exact(2)
            .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
            .collect();
        assert_eq!(values, [0, 1, 0x1234, 65535]);
        assert_ne!(plane.as_slice().as_ptr(), wire.as_ptr());
        assert_eq!(quota.snapshot().worker_bytes, wire.len());
        assert_eq!(wire, [0, 0, 1, 0, 0x34, 0x12, 0xff, 0xff]);
        drop(plane);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }
    #[test]
    fn conversion_refuses_original_quota_exhaustion_without_retained_charge() {
        let quota = quota(3);
        assert!(matches!(
            native_region_plane(&[1, 0, 2, 0], &quota, true),
            Err(AnimationError::Budget(_))
        ));
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }
    #[test]
    fn odd_plane_is_refused_before_conversion_admission() {
        let quota = quota(64);
        assert!(matches!(
            native_region_plane(&[1, 0, 2], &quota, true),
            Err(AnimationError::Runtime(_))
        ));
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }
    #[test]
    fn cancelled_conversion_refuses_before_storage_admission() {
        let quota = quota(64);
        assert!(matches!(
            native_region_plane_with_cancel(&[1, 0], &quota, true, || true),
            Err(AnimationError::Runtime(_))
        ));
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }
    #[test]
    fn mid_conversion_cancellation_releases_original_admission() {
        let quota = quota(64);
        let calls = std::cell::Cell::new(0);
        let wire = [1, 0, 2, 0, 3, 0];
        let result = native_region_plane_with_cancel(&wire, &quota, true, || {
            let count = calls.get() + 1;
            calls.set(count);
            count >= 4
        });
        assert!(matches!(result, Err(AnimationError::Runtime(_))));
        assert_eq!(quota.snapshot().worker_bytes, 0);
        assert_eq!(wire, [1, 0, 2, 0, 3, 0]);
    }
    fn encoded_fixture(quota: &QuotaGroup) -> EncodedRegion<StorageAdmission> {
        let properties = BTreeMap::<String, String>::new();
        let source = collect_region(
            RegionSpec {
                origin: [0, 0, 0],
                size: [2, 1, 1],
                max_bytes: 4096,
            },
            RegionLimits {
                cells: 2,
                palette: 1,
                work: 4096,
            },
            || false,
            |bytes| {
                quota
                    .reserve_external_storage(bytes)
                    .map_err(|_| RegionError::Admission)
            },
            |_| {
                Ok(Some(BlockStateView {
                    name: "minecraft:stone",
                    properties: &properties,
                }))
            },
        )
        .unwrap();
        let identity = "a".repeat(64);
        encode_region(
            source,
            &identity,
            4096,
            4096,
            || false,
            |bytes| {
                quota
                    .reserve_external_storage(bytes)
                    .map_err(|_| RegionError::Admission)
            },
        )
        .unwrap()
    }

    #[test]
    fn destination_copy_cancellation_drops_completed_result_and_restores_original_root() {
        let quota = quota(1024 * 1024);
        let encoded = encoded_fixture(&quota);
        let encoded_only = quota.snapshot().worker_bytes;
        assert!(encoded_only > 0);
        let stop = StopToken::default();
        let admission_seen = Cell::new(false);
        let immutable_result_seen = Cell::new(false);
        let result_pointer = Cell::new(None::<usize>);
        let result_binary_bytes = Cell::new(0_usize);
        let outcome = copy_region_response_with_destination_hooks_for_test(
            &encoded,
            &EngineLimits::default(),
            quota.clone(),
            &stop,
            |admission| {
                assert!(admission.shares_root(&quota));
                assert!(quota.snapshot().worker_bytes > encoded_only);
                admission_seen.set(true);
                stop.stop();
                Ok(())
            },
            |result| {
                assert!(stop.is_stopped());
                assert!(result.shares_root(&quota));
                assert_eq!(result.binary_bytes(), encoded.blocks().len());
                result_pointer.set(Some(result.planes()["b0"].as_ptr() as usize));
                result_binary_bytes.set(result.binary_bytes());
                immutable_result_seen.set(true);
                assert!(quota.snapshot().worker_bytes > encoded_only);
            },
        );
        assert!(matches!(outcome, Err(AnimationError::Runtime(_))));
        assert!(admission_seen.get());
        assert!(immutable_result_seen.get());
        assert!(result_pointer.get().is_some());
        assert_eq!(result_binary_bytes.get(), encoded.blocks().len());
        assert_eq!(quota.snapshot().worker_bytes, encoded_only);
        assert_eq!(encoded.blocks().len(), 4);
        drop(encoded);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }
}
