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
        || encoded.blocks().len() % 2 != 0
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
    ServiceValue::copy_from_borrowed_host(
        &metadata,
        &arrays,
        &BTreeMap::from([("b0".to_owned(), encoded.blocks())]),
        limits,
        quota,
    )
}
