//! Copy an admitted native region into an independently owned V8 completion.
//! Source identity is metadata; only the original admission establishes custody.
use crate::{
    engine::{ArraySpec, EngineLimits, ServiceValue, TypedArrayKind},
    error::{AnimationError, Result},
    world_region_encoding::EncodedRegion,
};
use ilium_execution::{QuotaGroup, StorageAdmission};
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
fn native_region_plane<'a>(
    wire: &'a [u8],
    quota: &QuotaGroup,
    big_endian: bool,
) -> Result<NativeRegionPlane<'a>> {
    if !wire.len().is_multiple_of(2) {
        return Err(AnimationError::Runtime(
            "incomplete native region index".into(),
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
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(wire.len())
        .map_err(|_| AnimationError::Budget("native region byte-order allocation".into()))?;
    for pair in wire.chunks_exact(2) {
        bytes.extend_from_slice(&[pair[1], pair[0]]);
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
    // Reject foreign custody before parsing or debiting the receiving bank.
    if !encoded.shares_root(&quota) {
        return Err(AnimationError::PermissionDenied(
            "foreign native region quota".into(),
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
    let plane = native_region_plane(encoded.blocks(), &quota, cfg!(target_endian = "big"))?;
    ServiceValue::copy_from_borrowed_host(
        &metadata,
        &arrays,
        &BTreeMap::from([("b0".to_owned(), plane.as_slice())]),
        limits,
        quota,
    )
}

#[cfg(test)]
mod region_plane_tests {
    use super::*;
    use ilium_execution::QuotaLimits;
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
}
