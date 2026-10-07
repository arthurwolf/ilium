//! Bounded textured mesh rendering for the public `host.gpu` facade.
//!
//! The current backend is deliberately CPU based: it uses the same admitted
//! media rasterizer as native image work, so a package gets identical image
//! handles and quota custody whether a GPU device is available or not.  The
//! wire contract keeps the camera plane and indexed geometry explicit, making
//! a hardware backend replaceable without changing package code.
use crate::{
    engine::{CompletionState, HostRequest, ServiceValue, TypedArrayKind},
    error::{AnimationError, Result},
    native_draw_host::NativeDrawHost,
    native_media::{ImageHandle, MeshTriangle, MeshVertex},
    runtime::PackageInstance,
};
use ilium_execution::{QuotaGroup, StorageAdmission};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

const MAX_TRIANGLES: usize = 2048;
const MAX_PENDING: usize = 32;
const REGISTRY_BYTES: usize = 64 * 1024;

struct Pending {
    request: HostRequest,
    value: ServiceValue,
    image: ImageHandle,
    close: bool,
}

pub struct NativeGpuHost {
    quota: QuotaGroup,
    closed: bool,
    owned: BTreeSet<u64>,
    pending: BTreeMap<u64, Pending>,
    _metadata: StorageAdmission,
}

fn invalid(message: &str) -> AnimationError {
    AnimationError::Runtime(format!("native gpu: {message}"))
}

fn fields(request: &HostRequest) -> Result<&serde_json::Map<String, Value>> {
    let fields = request
        .payload
        .metadata()
        .as_object()
        .ok_or_else(|| invalid("options must be a record"))?;
    let allowed = [
        "width",
        "height",
        "vertices",
        "indices",
        "camera",
        "texture",
        "texcoords",
    ];
    if fields.len() > allowed.len() || fields.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(invalid("unknown render option"));
    }
    Ok(fields)
}

fn dimension(fields: &serde_json::Map<String, Value>, name: &str) -> Result<u32> {
    fields
        .get(name)
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| (1..=4096).contains(value))
        .ok_or_else(|| invalid("render dimensions"))
}

fn marker(
    fields: &serde_json::Map<String, Value>,
    name: &str,
    required: bool,
) -> Result<Option<String>> {
    let Some(value) = fields.get(name) else {
        if required {
            return Err(invalid("required render plane missing"));
        }
        return Ok(None);
    };
    let marker = value
        .as_object()
        .filter(|value| value.len() == 1)
        .and_then(|value| value.get("$ilium_binary"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && value.len() <= 64)
        .ok_or_else(|| invalid("render plane marker"))?;
    Ok(Some(marker.to_owned()))
}

fn plane_f32(request: &HostRequest, marker: &str) -> Result<Vec<f32>> {
    let array = request
        .payload
        .arrays()
        .iter()
        .find(|array| array.name == marker)
        .ok_or_else(|| invalid("render float plane descriptor"))?;
    if array.kind != TypedArrayKind::F32 {
        return Err(invalid("render float plane kind"));
    }
    let bytes = request
        .payload
        .planes()
        .get(marker)
        .filter(|bytes| bytes.len() == array.elements.saturating_mul(4))
        .ok_or_else(|| invalid("render float plane bytes"))?;
    Ok(bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_ne_bytes(chunk.try_into().expect("chunk width")))
        .collect())
}

fn plane_u32(request: &HostRequest, marker: &str) -> Result<Vec<u32>> {
    let array = request
        .payload
        .arrays()
        .iter()
        .find(|array| array.name == marker)
        .ok_or_else(|| invalid("render index plane descriptor"))?;
    if array.kind != TypedArrayKind::U32 {
        return Err(invalid("render index plane kind"));
    }
    let bytes = request
        .payload
        .planes()
        .get(marker)
        .filter(|bytes| bytes.len() == array.elements.saturating_mul(4))
        .ok_or_else(|| invalid("render index plane bytes"))?;
    Ok(bytes
        .chunks_exact(4)
        .map(|chunk| u32::from_ne_bytes(chunk.try_into().expect("chunk width")))
        .collect())
}

fn handle_id(fields: &serde_json::Map<String, Value>) -> Result<Option<&str>> {
    let Some(value) = fields.get("texture") else {
        return Ok(None);
    };
    let value = value
        .as_object()
        .filter(|value| {
            value.len() == 2 && value.get("kind").and_then(Value::as_str) == Some("image")
        })
        .ok_or_else(|| invalid("texture handle"))?;
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty() && id.len() <= 128)
        .ok_or_else(|| invalid("texture handle id"))?;
    Ok(Some(id))
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
        "sha256": format!("{hash:x}"),
    }))
}

/// Column-major world-to-clip transform, followed by homogeneous division.
/// Pixel coordinates use a top-left origin; smaller normalized depth is nearer.
fn project_vertex(vertex: [f32; 3], camera: &[f32], width: u32, height: u32) -> Result<MeshVertex> {
    let [x, y, z] = vertex;
    let transform =
        |row: usize| camera[row] * x + camera[4 + row] * y + camera[8 + row] * z + camera[12 + row];
    let w = transform(3);
    if !w.is_finite() || w <= f32::EPSILON {
        return Err(invalid(
            "vertex behind camera or invalid homogeneous coordinate",
        ));
    }
    let projected = [transform(0) / w, transform(1) / w, transform(2) / w];
    if projected.iter().any(|value| !value.is_finite()) {
        return Err(invalid("nonfinite projected vertex"));
    }
    Ok(MeshVertex {
        x: (projected[0] + 1.0) * 0.5 * width as f32,
        y: (1.0 - projected[1]) * 0.5 * height as f32,
        depth: (projected[2] + 1.0) * 0.5,
        uv: [0.0, 0.0],
    })
}

impl NativeGpuHost {
    pub fn new(quota: QuotaGroup) -> Result<Self> {
        let metadata = quota
            .reserve_external_storage(REGISTRY_BYTES)
            .map_err(|error| AnimationError::Budget(format!("gpu registry: {error:?}")))?;
        Ok(Self {
            quota,
            closed: false,
            owned: BTreeSet::new(),
            pending: BTreeMap::new(),
            _metadata: metadata,
        })
    }

    pub fn revoke(&mut self) {
        self.closed = true;
        for pending in self.pending.values() {
            pending.request.stop_token().stop();
        }
        self.pending.clear();
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

    fn complete_pending(
        &mut self,
        instance: &mut PackageInstance,
        drawing: &mut NativeDrawHost,
        id: u64,
    ) -> Result<()> {
        let Some(pending) = self.pending.get(&id) else {
            return Ok(());
        };
        match instance.complete_baseline_media(&pending.request, pending.value.clone())? {
            CompletionState::Delivered => {
                let pending = self
                    .pending
                    .remove(&id)
                    .ok_or_else(|| invalid("gpu completion custody lost"))?;
                if pending.close {
                    self.owned.remove(&pending.image.id());
                    drawing.close_image(pending.image)?;
                }
            }
            CompletionState::Unknown | CompletionState::TimedOut | CompletionState::Cancelled => {
                let pending = self
                    .pending
                    .remove(&id)
                    .ok_or_else(|| invalid("gpu terminal custody lost"))?;
                // An unacknowledged close does not prove that the script has
                // discarded an already published image. Retire it with the
                // helper; only an unpublished render result can be reclaimed.
                if !pending.close {
                    self.owned.remove(&pending.image.id());
                    drawing.close_image(pending.image)?;
                }
            }
        }
        Ok(())
    }

    fn render_image(
        &mut self,
        request: &HostRequest,
        drawing: &mut NativeDrawHost,
    ) -> Result<ImageHandle> {
        let options = fields(request)?;
        let width = dimension(options, "width")?;
        let height = dimension(options, "height")?;
        let vertices_marker = marker(options, "vertices", true)?.expect("required marker");
        // Own a scratch admission before decoding planes or constructing geometry.
        let plane_bytes = request
            .payload
            .arrays()
            .iter()
            .try_fold(0usize, |bytes, array| {
                bytes.checked_add(array.elements.checked_mul(4)?)
            })
            .ok_or_else(|| invalid("render scratch overflow"))?;
        let scratch_bytes = plane_bytes
            .checked_mul(4)
            .and_then(|bytes| bytes.checked_add(MAX_TRIANGLES * 256))
            .ok_or_else(|| invalid("render scratch overflow"))?;
        let _scratch = self
            .quota
            .reserve_external_storage(scratch_bytes)
            .map_err(|error| AnimationError::Budget(format!("gpu geometry: {error:?}")))?;
        let vertices = plane_f32(request, &vertices_marker)?;
        if vertices.len() % 3 != 0
            || vertices.is_empty()
            || vertices.len() > MAX_TRIANGLES * 9
            || vertices.iter().any(|value| !value.is_finite())
        {
            return Err(invalid("vertex shape"));
        }
        let indices_marker = marker(options, "indices", false)?;
        let indices = match indices_marker {
            Some(marker) => plane_u32(request, &marker)?,
            None => (0..u32::try_from(vertices.len() / 3).map_err(|_| invalid("vertex count"))?)
                .collect(),
        };
        if indices.is_empty() || indices.len() % 3 != 0 || indices.len() / 3 > MAX_TRIANGLES {
            return Err(invalid("index shape or triangle limit"));
        }
        if indices
            .iter()
            .any(|index| usize::try_from(*index).map_or(true, |index| index >= vertices.len() / 3))
        {
            return Err(invalid("index range"));
        }
        let camera_marker = marker(options, "camera", true)?.expect("required marker");
        let camera = plane_f32(request, &camera_marker)?;
        if camera.len() != 16 || camera.iter().any(|value| !value.is_finite()) {
            return Err(invalid("camera matrix"));
        }
        let mut projected = vertices
            .chunks_exact(3)
            .map(|vertex| project_vertex([vertex[0], vertex[1], vertex[2]], &camera, width, height))
            .collect::<Result<Vec<_>>>()?;
        if let Some(texcoords_marker) = marker(options, "texcoords", false)? {
            let texcoords = plane_f32(request, &texcoords_marker)?;
            if texcoords.len() != projected.len() * 2
                || texcoords.iter().any(|value| !value.is_finite())
            {
                return Err(invalid("texture coordinate shape or nonfinite value"));
            }
            for (vertex, uv) in projected.iter_mut().zip(texcoords.chunks_exact(2)) {
                vertex.uv = [uv[0], uv[1]];
            }
        }
        let texture = match handle_id(options)? {
            Some(key) => {
                // Allocation IDs are lookup data, not publication authority.
                // Borrow the producer's existing registry entry without taking
                // responsibility for closing the producer's image.
                let handle = drawing
                    .registered_image_handle(key)
                    .or_else(|| drawing.source_image_handle(key))
                    .or_else(|| drawing.video_image_handle(key))
                    .ok_or_else(|| {
                        AnimationError::PermissionDenied("unpublished GPU texture".into())
                    })?;
                drawing.media().snapshot(handle)?;
                handle
            }
            None => drawing.media_mut().solid_image([255, 255, 255, 255])?,
        };
        let triangles = indices
            .chunks_exact(3)
            .enumerate()
            .map(|(rank, indices)| {
                let vertex = |index: u32| projected[usize::try_from(index).expect("checked index")];
                MeshTriangle {
                    vertices: [vertex(indices[0]), vertex(indices[1]), vertex(indices[2])],
                    texture,
                    sort_key: rank as u64,
                }
            })
            .collect::<Vec<_>>();
        let owned_texture = options.get("texture").is_none();
        let output =
            drawing
                .media_mut()
                .mesh_image(width, height, &triangles, &request.stop_token());
        if owned_texture {
            let _ = drawing.media_mut().close(texture);
        }
        let output = output?;
        Ok(output)
    }

    pub fn dispatch(
        &mut self,
        instance: &mut PackageInstance,
        drawing: &mut NativeDrawHost,
        request: HostRequest,
    ) -> Result<Option<HostRequest>> {
        if request.method != "gpu.render" && request.method != "media.images.close" {
            return Ok(Some(request));
        }
        if self.closed || request.is_cancelled() || !request.payload.shares_root(&self.quota) {
            return Err(AnimationError::PermissionDenied(
                "gpu owner retired or foreign".into(),
            ));
        }
        instance.check_baseline_media(&request)?;
        if request.method == "gpu.render" {
            instance.check_gpu_permission()?;
        }
        for id in self.pending.keys().copied().collect::<Vec<_>>() {
            self.complete_pending(instance, drawing, id)?;
        }
        if self.pending.len() >= MAX_PENDING {
            return Err(AnimationError::Budget("gpu pending inventory".into()));
        }
        if self.pending.contains_key(&request.id) {
            return Err(invalid("duplicate gpu completion"));
        }
        if request.method == "media.images.close" {
            let options = request
                .payload
                .metadata()
                .as_object()
                .ok_or_else(|| invalid("image close options"))?;
            if options.len() != 2 || options.get("kind").and_then(Value::as_str) != Some("image") {
                return Ok(Some(request));
            }
            let Some(id) = options
                .get("id")
                .and_then(Value::as_str)
                .and_then(|id| id.parse::<u64>().ok())
                .filter(|id| self.owned.contains(id))
            else {
                return Ok(Some(request));
            };
            let image = ImageHandle::from_id(id);
            let value = ServiceValue::copy_from_host(
                &json!({"ok": true, "value": null}),
                &[],
                &BTreeMap::new(),
                instance.engine_limits(),
                self.quota.clone(),
            )?;
            let request_id = request.id;
            self.pending.insert(
                request_id,
                Pending {
                    request,
                    value,
                    image,
                    close: true,
                },
            );
            self.complete_pending(instance, drawing, request_id)?;
            return Ok(None);
        }
        let image = self.render_image(&request, drawing)?;
        if let Err(error) = drawing.register_image(instance, &request, image) {
            let _ = drawing.close_image(image);
            return Err(error);
        }
        self.owned.insert(image.id());
        let value = ServiceValue::copy_from_host(
            &json!({"ok": true, "value": image_value(drawing, image)?}),
            &[],
            &BTreeMap::new(),
            instance.engine_limits(),
            self.quota.clone(),
        )?;
        let request_id = request.id;
        self.pending.insert(
            request_id,
            Pending {
                request,
                value,
                image,
                close: false,
            },
        );
        self.complete_pending(instance, drawing, request_id)?;
        Ok(None)
    }
}

#[cfg(test)]
mod projection_tests {
    use super::*;
    use crate::engine::{ArraySpec, EngineLimits, ServiceAuthority, ServiceBudget, ServicePhase};
    use crate::native_draw::DrawLimits;
    use crate::native_media::MediaLimits;
    use ilium_execution::QuotaLimits;
    use ilium_platform::owned_worker::StopToken;

    const IDENTITY: [f32; 16] = [
        1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
    ];

    #[test]
    fn camera_projects_column_major_translation_and_homogeneous_division() {
        let center = project_vertex([0., 0., 0.], &IDENTITY, 100, 80).unwrap();
        assert_eq!([center.x, center.y, center.depth], [50., 40., 0.5]);
        let corner = project_vertex([-1., 1., -1.], &IDENTITY, 100, 80).unwrap();
        assert_eq!([corner.x, corner.y, corner.depth], [0., 0., 0.]);
        let mut camera = IDENTITY;
        camera[12] = 1.;
        camera[15] = 2.;
        let moved = project_vertex([0., 0., 0.], &camera, 100, 80).unwrap();
        assert_eq!([moved.x, moved.y, moved.depth], [75., 40., 0.5]);
    }

    #[test]
    fn camera_refuses_behind_eye_and_nonfinite_coordinates() {
        let mut camera = IDENTITY;
        camera[15] = 0.;
        assert!(project_vertex([0., 0., 0.], &camera, 100, 80).is_err());
        camera[15] = -1.;
        assert!(project_vertex([0., 0., 0.], &camera, 100, 80).is_err());
        assert!(project_vertex([f32::INFINITY, 0., 0.], &IDENTITY, 100, 80).is_err());
    }

    fn mesh_request(quota: &QuotaGroup, texture: ImageHandle, texcoords: &[f32]) -> HostRequest {
        let limits = EngineLimits::default();
        let budget = ServiceBudget::new(&limits);
        let vertices: [f32; 9] = [-1., 1., 0., 1., 1., 0., -1., -1., 0.];
        let indices: [u32; 3] = [0, 1, 2];
        let floats = |values: &[f32]| {
            values
                .iter()
                .flat_map(|value| value.to_ne_bytes())
                .collect::<Vec<u8>>()
        };
        let arrays = [
            ArraySpec {
                name: "b0".into(),
                kind: TypedArrayKind::F32,
                elements: vertices.len(),
            },
            ArraySpec {
                name: "b1".into(),
                kind: TypedArrayKind::U32,
                elements: indices.len(),
            },
            ArraySpec {
                name: "b2".into(),
                kind: TypedArrayKind::F32,
                elements: IDENTITY.len(),
            },
            ArraySpec {
                name: "b3".into(),
                kind: TypedArrayKind::F32,
                elements: texcoords.len(),
            },
        ];
        let planes = BTreeMap::from([
            ("b0".into(), floats(&vertices)),
            (
                "b1".into(),
                indices
                    .iter()
                    .flat_map(|value| value.to_ne_bytes())
                    .collect(),
            ),
            ("b2".into(), floats(&IDENTITY)),
            ("b3".into(), floats(texcoords)),
        ]);
        let metadata = json!({
            "width":8, "height":8,
            "vertices":{"$ilium_binary":"b0"},
            "indices":{"$ilium_binary":"b1"},
            "camera":{"$ilium_binary":"b2"},
            "texcoords":{"$ilium_binary":"b3"},
            "texture":{"kind":"image","id":texture.id().to_string()},
        });
        let payload = ServiceValue::copy_request_from_host(
            &metadata,
            &arrays,
            &planes,
            &limits,
            quota.clone(),
            &budget,
        )
        .unwrap();
        HostRequest::from_transport(
            1,
            "gpu.render".into(),
            60_000,
            "a".repeat(64),
            ServiceAuthority {
                instance_id: 1,
                plan_generation: 1,
                authorization_epoch: 1,
            },
            ServicePhase::Async,
            payload,
        )
        .unwrap()
    }

    #[test]
    fn texture_refuses_allocated_but_unpublished_image_without_custody_transfer() {
        // A native allocation is not publication under the package authority.
        // Use the real media owner and renderer, with synthetic mesh geometry.
        let quota = QuotaGroup::new(QuotaLimits {
            clients: 0,
            jobs: 0,
            service_jobs: 0,
            input_bytes: 0,
            result_bytes: 0,
            worker_threads: 0,
            worker_bytes: 16 * 1024 * 1024,
        });
        let mut drawing =
            NativeDrawHost::new(quota.clone(), MediaLimits::default(), DrawLimits::default())
                .unwrap();
        let texture = drawing.media_mut().solid_image([0, 255, 0, 255]).unwrap();
        let mut host = NativeGpuHost::new(quota.clone()).unwrap();
        let request = mesh_request(&quota, texture, &[0., 0., 0., 0., 0., 0.]);
        let before = quota.snapshot().worker_bytes;
        let result = host.render_image(&request, &mut drawing);
        let refused = matches!(result, Err(AnimationError::PermissionDenied(_)));
        // Retire the unexpected result on the old implementation before the
        // assertion so the RED run does not manufacture a quota leak.
        if let Ok(image) = result {
            drawing.media_mut().close(image).unwrap();
        }
        assert_eq!(quota.snapshot().worker_bytes, before);
        assert!(
            refused,
            "GPU texture sampling must reject an allocated but unpublished image"
        );
        // Reading must never acquire the producer's close custody.
        assert!(host.owned.is_empty());
        assert!(drawing.media().snapshot(texture).is_ok());
        drawing.media_mut().close(texture).unwrap();
        drop(request);
        drop(host);
        drop(drawing);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }

    #[test]
    fn textured_mesh_uses_requested_uvs_and_refuses_malformed_planes_without_leaks() {
        // Synthetic codec/geometry fixture exercises the production renderer;
        // direct rendering does not claim broker or helper qualification.
        let quota = QuotaGroup::new(QuotaLimits {
            clients: 0,
            jobs: 0,
            service_jobs: 0,
            input_bytes: 0,
            result_bytes: 0,
            worker_threads: 0,
            worker_bytes: 512 * 1024 * 1024,
        });
        let mut drawing =
            NativeDrawHost::new(quota.clone(), MediaLimits::default(), DrawLimits::default())
                .unwrap();
        let fixture = image::ImageBuffer::from_fn(2, 1, |x, _| {
            image::Rgba(if x == 0 {
                [255u8, 0, 0, 255]
            } else {
                [0, 255, 0, 255]
            })
        });
        let mut encoded = std::io::Cursor::new(Vec::new());
        fixture
            .write_to(&mut encoded, image::ImageFormat::Png)
            .unwrap();
        let texture = drawing
            .media_mut()
            .decode(encoded.get_ref(), &StopToken::default())
            .unwrap();
        drawing.register_test_image(texture);
        let mut host = NativeGpuHost::new(quota.clone()).unwrap();
        let request = mesh_request(&quota, texture, &[1., 0., 1., 0., 1., 0.]);
        let image = host.render_image(&request, &mut drawing).unwrap();
        let pixels = drawing.media().snapshot(image).unwrap();
        assert!(pixels
            .view()
            .rgba
            .chunks_exact(4)
            .any(|pixel| pixel == [0, 255, 0, 255]));
        assert!(!pixels
            .view()
            .rgba
            .chunks_exact(4)
            .any(|pixel| pixel == [255, 0, 0, 255]));
        drawing.media_mut().close(image).unwrap();
        drop(pixels);
        drop(request);
        for malformed in [&[0., 0.][..], &[0., 0., 1., 0., 0., f32::NAN][..]] {
            let request = mesh_request(&quota, texture, malformed);
            let before = quota.snapshot().worker_bytes;
            assert!(host.render_image(&request, &mut drawing).is_err());
            assert_eq!(quota.snapshot().worker_bytes, before);
        }
        drawing.close_image(texture).unwrap();
        let expired = mesh_request(&quota, texture, &[1., 0., 1., 0., 1., 0.]);
        let before = quota.snapshot().worker_bytes;
        assert!(matches!(
            host.render_image(&expired, &mut drawing),
            Err(AnimationError::PermissionDenied(_))
        ));
        assert_eq!(quota.snapshot().worker_bytes, before);
        drop(expired);
        drop(host);
        drop(drawing);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }
}
