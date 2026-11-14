//! Render immutable multipart entity faces in the same bank and depth frame as
//! terrain and fluids. Pixel color comes only from the decoded bound atlas.
use super::{
    assets::{
        bank::TextureBank,
        budget::{ByteBudget, Cancel},
        error::{AssetError, Result},
        texture::{LinearRgba, Texture},
    },
    surface_entity_binding::{EntityFace, PreparedEntityMesh},
    surface_raster::{DirectionalLight, FragmentShader, RasterFrame, Vertex},
};
use std::time::Duration;

struct EntityShader<'a> {
    texture: &'a Texture,
    time: Duration,
    shade: f32,
}
impl FragmentShader for EntityShader<'_> {
    fn sample(&self, uv: [f32; 2]) -> Result<LinearRgba> {
        let color = self.texture.sample_color(uv, self.time).ok_or_else(|| {
            AssetError::InvalidMetadata("entity atlas sample outside decoded color frame".into())
        })?;
        LinearRgba::from_straight(
            color
                .straight()
                .map(|channel| (channel * self.shade).min(1.0)),
            color.alpha(),
        )
        .ok_or_else(|| AssetError::InvalidMetadata("invalid entity lighting result".into()))
    }
}

fn project(face: &EntityFace, camera: [f64; 3], scale: f64, size: [usize; 2]) -> [Vertex; 4] {
    std::array::from_fn(|index| {
        project_point(
            face.anchor,
            face.points[index],
            face.uv[index],
            camera,
            scale,
            size,
        )
    })
}
fn project_point(
    anchor: [i32; 3],
    point: [f32; 3],
    uv: [f32; 2],
    camera: [f64; 3],
    scale: f64,
    size: [usize; 2],
) -> Vertex {
    let base = camera.map(|value| value.floor() as i64);
    let local_camera = std::array::from_fn::<_, 3, _>(|axis| camera[axis] - base[axis] as f64);
    let p = std::array::from_fn::<_, 3, _>(|axis| {
        let center = if axis < 2 { 0.5 } else { 0.0 };
        (i64::from(anchor[axis]) - base[axis]) as f64 + f64::from(point[axis]) + center
            - local_camera[axis]
    });
    Vertex {
        x: size[0] as f64 * 0.5 + (p[0] - p[1]) * 0.8660254037844386 * scale,
        y: size[1] as f64 * 0.5 + ((p[0] + p[1]) * 0.5 - p[2]) * scale,
        depth: p.iter().sum(),
        uv,
    }
}

/// Compose after block faces and before the fluid pass. RasterFrame's
/// depth/order-independent alpha accounting decides visibility, not call order.
#[expect(
    clippy::too_many_arguments,
    reason = "bank, account, camera, frame time and cancellation are explicit snapshot contracts"
)]
pub fn draw_entity_mesh(
    mesh: &PreparedEntityMesh,
    bank: &TextureBank,
    budget: &ByteBudget,
    camera: [f64; 3],
    scale: f64,
    time: Duration,
    light: DirectionalLight,
    output: &mut RasterFrame,
    cancel: Cancel<'_>,
) -> Result<()> {
    cancel.check()?;
    if mesh.bank != bank.identity()
        || !mesh.uses_budget(budget)
        || !output.uses_budget(budget)
        || !output.is_valid()
        || !scale.is_finite()
        || !(0.01..=1024.0).contains(&scale)
        || camera
            .iter()
            .any(|coordinate| !coordinate.is_finite() || coordinate.abs() > f64::from(i32::MAX))
    {
        return Err(AssetError::InvalidMetadata(
            "entity frame/bank/account/camera mismatch".into(),
        ));
    }
    for face in &mesh.faces {
        cancel.check()?;
        let texture = bank.texture(face.texture).ok_or_else(|| {
            AssetError::InvalidMetadata("entity face references stale atlas".into())
        })?;
        let shade = light.factor(face.normal, true)?;
        let shader = EntityShader {
            texture,
            time,
            shade,
        };
        let vertices = project(face, camera, scale, output.size());
        output.quad(vertices, face.owner, face.alpha, &shader, cancel)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn projection_uses_camera_relative_world_coordinates() {
        let projected = project_point(
            [1_000_000, 1_000_000, 64],
            [0.0; 3],
            [0.0; 2],
            [1_000_000.0, 1_000_000.0, 64.0],
            4.0,
            [100, 100],
        );
        assert_eq!(
            [projected.x, projected.y, projected.depth],
            [50.0, 52.0, 1.0]
        );
    }
}
