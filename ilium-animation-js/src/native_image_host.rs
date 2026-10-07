//! Native image facade for the existing animation worker. The same registry backs
//! SDK handles and prepared draw commands. Pure decoding runs outside the broker
//! mutex; the original helper request and result admission survive its copy ACK.
use crate::{
    engine::{ArraySpec, CompletionState, HostRequest, ServiceValue, TypedArrayKind},
    error::{AnimationError, Result},
    native_draw_host::NativeDrawHost,
    native_media::{ImageHandle, Sampling},
    runtime::PackageInstance,
};
use ilium_execution::{QuotaGroup, StorageAdmission};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

const REGISTRY_BYTES: usize = 64 * 1024;
const MAX_OWNED: usize = 64;
const MAX_PENDING: usize = 64;
const MAX_SAMPLE_BYTES: usize = 16 * 1024 * 1024;

fn invalid(message: &'static str) -> AnimationError {
    AnimationError::Runtime(format!("native image: {message}"))
}
fn fields<'a>(
    request: &'a HostRequest,
    allowed: &[&str],
) -> Result<&'a serde_json::Map<String, Value>> {
    let fields = request
        .payload
        .metadata()
        .as_object()
        .ok_or_else(|| invalid("options must be a record"))?;
    if fields.len() > allowed.len() || fields.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(invalid("unknown image option"));
    }
    Ok(fields)
}
fn integer(fields: &serde_json::Map<String, Value>, name: &str, maximum: u64) -> Result<u32> {
    fields
        .get(name)
        .and_then(Value::as_u64)
        .filter(|value| (1..=maximum).contains(value))
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| invalid("image dimension or bound"))
}
fn handle_id<'a>(fields: &'a serde_json::Map<String, Value>, name: &str) -> Result<&'a str> {
    let value = fields
        .get(name)
        .and_then(Value::as_object)
        .ok_or_else(|| invalid("image handle record"))?;
    // V8 replaces the exact branded wrapper with its inert two-field projection.
    // The projection is lookup data only; owned below is the native authority.
    if value.len() != 2 || value.get("kind").and_then(Value::as_str) != Some("image") {
        return Err(invalid("image handle projection"));
    }
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty() && id.len() <= 128)
        .ok_or_else(|| invalid("image handle id"))?;
    Ok(id)
}
fn input_bytes<'a>(
    request: &'a HostRequest,
    fields: &serde_json::Map<String, Value>,
) -> Result<&'a [u8]> {
    let marker = fields
        .get("bytes")
        .and_then(Value::as_object)
        .and_then(|value| (value.len() == 1).then_some(value))
        .and_then(|value| value.get("$ilium_binary"))
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("image bytes marker"))?;
    let arrays = request.payload.arrays();
    if arrays.len() != 1
        || arrays[0].name != marker
        || arrays[0].kind != TypedArrayKind::U8
        || request.payload.planes().len() != 1
    {
        return Err(invalid("image bytes array inventory"));
    }
    request
        .payload
        .planes()
        .get(marker)
        .filter(|bytes| bytes.len() == arrays[0].elements)
        .map(Vec::as_slice)
        .ok_or_else(|| invalid("image bytes plane"))
}
fn no_planes(request: &HostRequest) -> Result<()> {
    if !request.payload.arrays().is_empty() || !request.payload.planes().is_empty() {
        return Err(invalid("unexpected image binary plane"));
    }
    Ok(())
}
fn image_value(drawing: &NativeDrawHost, handle: ImageHandle) -> Result<Value> {
    let image = drawing.media().snapshot(handle)?;
    let pixels = image.view();
    let hash = Sha256::digest(&pixels.rgba);
    Ok(json!({
        "id": handle.id().to_string(),
        "width": pixels.width,
        "height": pixels.height,
        "format": "rgba8",
        "sha256": format!("{hash:x}")
    }))
}
fn sample_value(
    request: &HostRequest,
    drawing: &NativeDrawHost,
    quota: &QuotaGroup,
    limits: &crate::engine::EngineLimits,
    image: ImageHandle,
) -> Result<ServiceValue> {
    no_planes(request)?;
    let options = fields(request, &["image", "rectangle", "format"])?;
    let _ = handle_id(options, "image")?;
    let snapshot = drawing.media().snapshot(image)?;
    let source = snapshot.view();
    let rectangle = options
        .get("rectangle")
        .and_then(Value::as_object)
        .filter(|rectangle| rectangle.len() == 4)
        .ok_or_else(|| invalid("sample rectangle"))?;
    let coordinate = |name: &str| -> Result<usize> {
        rectangle
            .get(name)
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .ok_or_else(|| invalid("sample rectangle coordinate"))
    };
    let x = coordinate("x")?;
    let y = coordinate("y")?;
    let width = coordinate("width")?;
    let height = coordinate("height")?;
    let source_width = source.width as usize;
    let source_height = source.height as usize;
    if width == 0
        || height == 0
        || x > source_width
        || y > source_height
        || width > source_width - x
        || height > source_height - y
    {
        return Err(invalid("sample outside original image"));
    }
    let format = options
        .get("format")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("sample format"))?;
    let (row_elements, kind) = match format {
        "mono1" => (width.div_ceil(8), TypedArrayKind::U8),
        "mono8" | "gray8" => (width, TypedArrayKind::U8),
        "gray32" => (width, TypedArrayKind::F32),
        "rgb8" => (
            width
                .checked_mul(3)
                .ok_or_else(|| invalid("sample width"))?,
            TypedArrayKind::U8,
        ),
        "rgba8" => (
            width
                .checked_mul(4)
                .ok_or_else(|| invalid("sample width"))?,
            TypedArrayKind::U8,
        ),
        _ => return Err(invalid("sample format")),
    };
    let elements = row_elements
        .checked_mul(height)
        .ok_or_else(|| invalid("sample size"))?;
    let bytes = elements
        .checked_mul(if kind == TypedArrayKind::F32 { 4 } else { 1 })
        .filter(|bytes| {
            *bytes <= MAX_SAMPLE_BYTES && *bytes <= limits.pending_bytes.saturating_sub(32 * 1024)
        })
        .ok_or_else(|| {
            AnimationError::Budget("image sample exceeds original service envelope".into())
        })?;
    let _admission = quota
        .reserve_external_storage(bytes + 1024)
        .map_err(|error| AnimationError::Budget(format!("image sample admission: {error:?}")))?;
    let mut output = vec![0u8; bytes];
    for row in 0..height {
        if request.is_cancelled() {
            return Err(invalid("sample cancelled"));
        }
        for column in 0..width {
            let offset = ((y + row) * source_width + x + column) * 4;
            let pixel = &source.rgba[offset..offset + 4];
            let alpha = pixel[3] as f32 / 255.0;
            let luminance =
                (0.2126 * pixel[0] as f32 + 0.7152 * pixel[1] as f32 + 0.0722 * pixel[2] as f32)
                    * alpha;
            match format {
                "mono1" => {
                    if luminance >= 127.5 {
                        output[row * row_elements + column / 8] |= 0x80 >> (column % 8);
                    }
                }
                "mono8" => output[row * row_elements + column] = u8::from(luminance >= 127.5),
                "gray8" => output[row * row_elements + column] = luminance.round() as u8,
                "gray32" => {
                    let offset = (row * row_elements + column) * 4;
                    output[offset..offset + 4].copy_from_slice(&(luminance / 255.0).to_le_bytes());
                }
                "rgb8" => {
                    let offset = row * row_elements + column * 3;
                    for channel in 0..3 {
                        output[offset + channel] = (pixel[channel] as f32 * alpha).round() as u8;
                    }
                }
                "rgba8" => {
                    let offset = row * row_elements + column * 4;
                    output[offset..offset + 4].copy_from_slice(pixel);
                }
                _ => unreachable!("validated sample format"),
            }
        }
    }
    let mut planes = BTreeMap::new();
    planes.insert("b0".to_owned(), output);
    let arrays = [ArraySpec {
        name: "b0".into(),
        kind,
        elements,
    }];
    ServiceValue::copy_from_host(
        &json!({"ok":true,"value":{"$ilium_binary":"b0"}}),
        &arrays,
        &planes,
        limits,
        quota.clone(),
    )
}

struct ImageCompletion {
    request: HostRequest,
    value: ServiceValue,
    opened: Option<ImageHandle>,
    closing: Option<ImageHandle>,
}
pub struct NativeImageHost {
    quota: QuotaGroup,
    closed: bool,
    owned: BTreeSet<u64>,
    pending: BTreeMap<u64, ImageCompletion>,
    _metadata: StorageAdmission,
}
impl NativeImageHost {
    pub fn new(quota: QuotaGroup) -> Result<Self> {
        let metadata = quota
            .reserve_external_storage(REGISTRY_BYTES)
            .map_err(|error| AnimationError::Budget(format!("image host admission: {error:?}")))?;
        Ok(Self {
            quota,
            closed: false,
            owned: BTreeSet::new(),
            pending: BTreeMap::new(),
            _metadata: metadata,
        })
    }
    pub fn revoke(&mut self) {
        // A copy error can mean the child accepted the descriptor before the
        // ACK was lost. Retain original request, result and image allocation.
        self.closed = true;
    }
    pub fn release_terminal_after_helper_retirement(&mut self, drawing: &mut NativeDrawHost) {
        if !self.closed {
            return;
        }
        self.pending.clear();
        for id in std::mem::take(&mut self.owned) {
            let _ = drawing.close_image(ImageHandle::from_id(id));
        }
    }
    pub fn is_drained(&self) -> bool {
        self.closed && self.pending.is_empty() && self.owned.is_empty()
    }
    fn owned_image(&self, handle: ImageHandle) -> Result<()> {
        if !self.owned.contains(&handle.id()) {
            return Err(AnimationError::PermissionDenied(
                "foreign native image".into(),
            ));
        }
        Ok(())
    }
    fn resolve_image(&self, drawing: &NativeDrawHost, id: &str) -> Result<ImageHandle> {
        if id.starts_with("source-image-") {
            // Source V2 registered the original admitted Arc under this exact
            // native owner. A copied string without that owner never resolves.
            return drawing.source_image_handle(id).ok_or_else(|| {
                AnimationError::PermissionDenied("unknown native source image".into())
            });
        }
        if id.starts_with("video-image-") {
            // The Video owner imported the decoder's original admitted Arc.
            // A guest-copied image ID without that owner never resolves.
            return drawing.video_image_handle(id).ok_or_else(|| {
                AnimationError::PermissionDenied("unknown native video image".into())
            });
        }
        let number = id
            .parse::<u64>()
            .ok()
            .filter(|number| *number != 0)
            .ok_or_else(|| invalid("generic image ID"))?;
        let handle = ImageHandle::from_id(number);
        if !self.owned.contains(&number) && drawing.registered_image_handle(id).is_none() {
            return Err(AnimationError::PermissionDenied(
                "foreign native image".into(),
            ));
        }
        // GPU images share the authoritative prepared registry but keep their
        // producer's close custody. Only sample/resize use this borrowed lookup.
        Ok(handle)
    }
    pub fn dispatch(
        &mut self,
        instance: &mut PackageInstance,
        drawing: &mut NativeDrawHost,
        request: HostRequest,
    ) -> Result<Option<HostRequest>> {
        if !matches!(
            request.method.as_str(),
            "media.images.decode"
                | "media.images.resize"
                | "media.images.sample"
                | "media.images.close"
        ) {
            return Ok(Some(request));
        }
        if self.closed || request.is_cancelled() || !request.payload.shares_root(&self.quota) {
            return Err(AnimationError::PermissionDenied(
                "image owner retired or foreign".into(),
            ));
        }
        if self.pending.len() >= MAX_PENDING {
            return Err(AnimationError::Budget(
                "native image pending inventory".into(),
            ));
        }
        // Brief native authority check. The decode, resize, sample and large-Arc
        // retirement below run on the existing physically supervised animation
        // worker, without holding PermissionBroker's mutex or the UI mailbox.
        instance.check_baseline_media(&request)?;
        let limits = instance.engine_limits().clone();
        let mut opened = None;
        let mut closing = None;
        let response = (|| -> Result<Option<ServiceValue>> {
            let value = match request.method.as_str() {
                "media.images.decode" => {
                    let options = fields(&request, &["bytes", "max_pixels", "width", "height"])?;
                    let max_pixels = integer(options, "max_pixels", 16 * 1024 * 1024)? as usize;
                    let bytes = input_bytes(&request, options)?;
                    let width = options
                        .get("width")
                        .map(|_| integer(options, "width", 16384))
                        .transpose()?;
                    let height = options
                        .get("height")
                        .map(|_| integer(options, "height", 16384))
                        .transpose()?;
                    if width.is_some() != height.is_some() {
                        return Err(invalid("decode resize requires both dimensions"));
                    }
                    if let (Some(width), Some(height)) = (width, height) {
                        if (width as usize)
                            .checked_mul(height as usize)
                            .is_none_or(|pixels| pixels > max_pixels)
                        {
                            return Err(AnimationError::Budget(
                                "resized image exceeds caller pixels".into(),
                            ));
                        }
                    }
                    let stop = request.stop_token();
                    let mut image = drawing
                        .media_mut()
                        .decode_bounded(bytes, max_pixels, &stop)?;
                    if let (Some(width), Some(height)) = (width, height) {
                        let resized = drawing.media_mut().resample(
                            image,
                            width,
                            height,
                            Sampling::Bilinear,
                            &stop,
                        );
                        let _ = drawing.media_mut().close(image);
                        image = resized?;
                    }
                    opened = Some(image);
                    if self.owned.len() >= MAX_OWNED {
                        return Err(AnimationError::Budget(
                            "native image handle inventory".into(),
                        ));
                    }
                    drawing.register_image(instance, &request, image)?;
                    if !self.owned.insert(image.id()) {
                        return Err(invalid("duplicate native image identity"));
                    }
                    image_value(drawing, image)?
                }
                "media.images.resize" => {
                    no_planes(&request)?;
                    let options = fields(&request, &["image", "width", "height", "filter"])?;
                    let source = self.resolve_image(drawing, handle_id(options, "image")?)?;
                    let width = integer(options, "width", 16384)?;
                    let height = integer(options, "height", 16384)?;
                    let filter = match options.get("filter").and_then(Value::as_str) {
                        Some("nearest") => Sampling::Nearest,
                        Some("linear") => Sampling::Bilinear,
                        _ => return Err(invalid("resize filter")),
                    };
                    let image = drawing.media_mut().resample(
                        source,
                        width,
                        height,
                        filter,
                        &request.stop_token(),
                    )?;
                    opened = Some(image);
                    if self.owned.len() >= MAX_OWNED {
                        return Err(AnimationError::Budget(
                            "native image handle inventory".into(),
                        ));
                    }
                    drawing.register_image(instance, &request, image)?;
                    if !self.owned.insert(image.id()) {
                        return Err(invalid("duplicate native image identity"));
                    }
                    image_value(drawing, image)?
                }
                "media.images.sample" => {
                    let options = fields(&request, &["image", "rectangle", "format"])?;
                    let image = self.resolve_image(drawing, handle_id(options, "image")?)?;
                    return sample_value(&request, drawing, &self.quota, &limits, image).map(Some);
                }
                "media.images.close" => {
                    no_planes(&request)?;
                    let options = fields(&request, &["id", "kind"])?;
                    if options.get("kind").and_then(Value::as_str) != Some("image") {
                        return Err(invalid("image close kind"));
                    }
                    let id = options
                        .get("id")
                        .and_then(Value::as_str)
                        .and_then(|id| id.parse::<u64>().ok())
                        .filter(|id| *id != 0)
                        .ok_or_else(|| invalid("image close id"))?;
                    let handle = ImageHandle::from_id(id);
                    self.owned_image(handle)?;
                    closing = Some(handle);
                    Value::Null
                }
                _ => unreachable!("recognized image method"),
            };
            ServiceValue::copy_from_host(
                &json!({"ok":true,"value":value}),
                &[],
                &BTreeMap::new(),
                &limits,
                self.quota.clone(),
            )
            .map(Some)
        })();
        let response = match response {
            Ok(Some(value)) => value,
            Ok(None) => return Ok(None),
            Err(error) => {
                if let Some(image) = opened {
                    self.owned.remove(&image.id());
                    let _ = drawing.close_image(image);
                }
                let code = match error {
                    AnimationError::Budget(_) => "budget_exceeded",
                    AnimationError::PermissionDenied(_) => "permission_denied",
                    _ => "image_failed",
                };
                ServiceValue::copy_from_host(
                    &json!({"ok":false,"error":{"code":code,"message":"Native image request failed."}}),
                    &[],
                    &BTreeMap::new(),
                    &limits,
                    self.quota.clone(),
                )?
            }
        };
        let id = request.id;
        if self.pending.contains_key(&id) {
            self.closed = true;
            return Err(invalid("duplicate original image completion"));
        }
        self.pending.insert(
            id,
            ImageCompletion {
                request,
                value: response,
                opened,
                closing,
            },
        );
        let result = {
            let pending = self
                .pending
                .get(&id)
                .ok_or_else(|| invalid("image completion custody lost"))?;
            instance.complete_baseline_media(&pending.request, pending.value.clone())
        };
        match result {
            Ok(CompletionState::Delivered) => {
                let pending = self
                    .pending
                    .remove(&id)
                    .ok_or_else(|| invalid("image delivered custody lost"))?;
                if let Some(image) = pending.closing {
                    drawing.close_image(image)?;
                    self.owned.remove(&image.id());
                }
            }
            Ok(
                CompletionState::Unknown | CompletionState::TimedOut | CompletionState::Cancelled,
            ) => {
                let pending = self
                    .pending
                    .remove(&id)
                    .ok_or_else(|| invalid("image terminal custody lost"))?;
                if let Some(image) = pending.opened {
                    self.owned.remove(&image.id());
                    drawing.close_image(image)?;
                }
            }
            Err(error) => {
                // A bounded refusal leaves the helper resolver pending; a pipe
                // failure can make descriptor delivery ambiguous. The actor
                // now fails closed. Only physical helper retirement releases
                // this exact request, ServiceValue and image allocation.
                self.closed = true;
                return Err(error);
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
#[path = "native_image_qualification_tests.rs"]
mod qualification_tests;
