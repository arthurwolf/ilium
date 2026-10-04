//! Pinned 1.19.3 built-in block-entity model recipes for saved beds, chests,
//! and the bell body added to the ordinary JSON bell stand.
//!
//! These are the exact ModelPart CubeListBuilder inputs and 64x64 atlas names,
//! not JSON block-model substitutes. A binder must still bake Cube/Polygon UVs,
//! apply the source transform in order, charge the shared bank/account, and
//! rasterize every face before an empty native JSON model counts as rendered.
use super::chunk::BlockState;
use crate::voxel_landscape::assets::identity::ResourceId;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cube {
    pub uv_origin: [u16; 2],
    /// Native ModelPart units, divided by 16 during Cube construction.
    pub origin: [f32; 3],
    pub size: [f32; 3],
    pub pose_translation: [f32; 3],
    /// Euler XYZ radians passed to PartPose.offsetAndRotation.
    pub pose_rotation: [f32; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BedPart {
    Head,
    Foot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChestPart {
    Single,
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Facing {
    North,
    South,
    West,
    East,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuiltinKind {
    Bed(BedPart),
    Chest(ChestPart),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Recipe {
    pub kind: BuiltinKind,
    pub facing: Facing,
    pub atlas: ResourceId,
    pub atlas_size: [u16; 2],
    /// Main, left leg, right leg for a bed; bottom, lid, lock for a chest.
    pub cubes: [Cube; 3],
    /// Saved-world BedRenderer.renderPiece: both head and foot use zero extra
    /// Z offset; the -1 offset belongs only to the world-null inventory preview.
    /// World branch then translates(0,.5625,0), rotates X90,
    /// translate(.5,.5,.5), rotate Z(180+facing.toYRot), translate(-.5,-.5,-.5).
    /// ChestRenderer: translate center, rotate Y(-facing angle), uncenter.
    pub transform: Transform,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BellBody {
    pub atlas: ResourceId,
    pub atlas_size: [u16; 2],
    /// Body and nested base in the static zero-ringing saved-world pose.
    pub cubes: [Cube; 2],
}

#[derive(Clone, Debug, PartialEq)]
pub struct BakedFace {
    /// Renderer axes [Java X, Java Z, Java Y], relative to the saved block.
    pub points: [[f64; 3]; 4],
    /// ModelPart$Polygon's vertex remap, divided by the native 64x64 atlas.
    pub uv: [[f32; 2]; 4],
    pub normal: [f32; 3],
    pub part: u16,
    pub face: u16,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Transform {
    Bed {
        facing_degrees: f32,
    },
    Chest {
        facing_degrees: f32,
        /// Static closed pose is a saved-scene policy; Java's live openness
        /// animation depends on runtime block entity state unavailable in NBT.
        openness: f32,
    },
    Bell,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("native builtin block state has incomplete or invalid properties")]
    Properties,
    #[error("native builtin atlas identifier: {0}")]
    Resource(#[from] crate::voxel_landscape::assets::AssetError),
}

fn cube(uv_origin: [u16; 2], origin: [f32; 3], size: [f32; 3]) -> Cube {
    Cube {
        uv_origin,
        origin,
        size,
        pose_translation: [0.0; 3],
        pose_rotation: [0.0; 3],
    }
}

fn parse_facing(value: Option<&String>) -> Result<Facing, Error> {
    match value.map(String::as_str) {
        Some("north") => Ok(Facing::North),
        Some("south") => Ok(Facing::South),
        Some("west") => Ok(Facing::West),
        Some("east") => Ok(Facing::East),
        _ => Err(Error::Properties),
    }
}

fn facing_degrees(facing: Facing) -> f32 {
    match facing {
        Facing::South => 0.0,
        Facing::West => 90.0,
        Facing::North => 180.0,
        Facing::East => 270.0,
    }
}

/// BedRenderer.createHeadLayer/createFootLayer exactly, including each leg's
/// independent UV offset and PartPose rotations. The static atlas color comes
/// from the saved block ID. Matching it to BedBlockEntity's stored dye still
/// requires an explicit saved-data check before claiming native parity.
pub fn bed(state: &BlockState) -> Result<Option<Recipe>, Error> {
    let Some(color) = state
        .name
        .strip_prefix("minecraft:")
        .and_then(|name| name.strip_suffix("_bed"))
    else {
        return Ok(None);
    };
    if !matches!(
        color,
        "white"
            | "orange"
            | "magenta"
            | "light_blue"
            | "yellow"
            | "lime"
            | "pink"
            | "gray"
            | "light_gray"
            | "cyan"
            | "purple"
            | "blue"
            | "brown"
            | "green"
            | "red"
            | "black"
    ) {
        return Err(Error::Properties);
    }
    if state.properties.len() != 3
        || !matches!(
            state.properties.get("occupied").map(String::as_str),
            Some("true" | "false")
        )
    {
        return Err(Error::Properties);
    }
    let part = match state.properties.get("part").map(String::as_str) {
        Some("head") => BedPart::Head,
        Some("foot") => BedPart::Foot,
        _ => return Err(Error::Properties),
    };
    let facing = parse_facing(state.properties.get("facing"))?;
    let main = cube(
        if part == BedPart::Head {
            [0, 0]
        } else {
            [0, 22]
        },
        [0.0; 3],
        [16.0, 16.0, 6.0],
    );
    let (left_uv, left_origin, left_rotation, right_uv, right_origin, right_rotation) =
        if part == BedPart::Head {
            (
                [50, 6],
                [0.0, 6.0, 0.0],
                [1.5707964, 0.0, 1.5707964],
                [50, 18],
                [-16.0, 6.0, 0.0],
                [1.5707964, 0.0, std::f32::consts::PI],
            )
        } else {
            (
                [50, 0],
                [0.0, 6.0, -16.0],
                [1.5707964, 0.0, 0.0],
                [50, 12],
                [-16.0, 6.0, -16.0],
                [1.5707964, 0.0, 4.712389],
            )
        };
    let mut left = cube(left_uv, left_origin, [3.0; 3]);
    left.pose_rotation = left_rotation;
    let mut right = cube(right_uv, right_origin, [3.0; 3]);
    right.pose_rotation = right_rotation;
    Ok(Some(Recipe {
        kind: BuiltinKind::Bed(part),
        facing,
        atlas: ResourceId::parse(&format!("minecraft:entity/bed/{color}"))?,
        atlas_size: [64, 64],
        cubes: [main, left, right],
        transform: Transform::Bed {
            facing_degrees: 180.0 + facing_degrees(facing),
        },
    }))
}

/// ChestRenderer.createSingle/DoubleRight/DoubleLeft factory cuboids. The
/// source renderer uses a runtime openness and December 24-26 atlas branch;
/// this static saved-scene recipe explicitly selects a closed ordinary chest.
pub fn chest(state: &BlockState) -> Result<Option<Recipe>, Error> {
    if state.name != "minecraft:chest" {
        return Ok(None);
    }
    if state.properties.len() != 3
        || !matches!(
            state.properties.get("waterlogged").map(String::as_str),
            Some("true" | "false")
        )
    {
        return Err(Error::Properties);
    }
    let part = match state.properties.get("type").map(String::as_str) {
        Some("single") => ChestPart::Single,
        Some("left") => ChestPart::Left,
        Some("right") => ChestPart::Right,
        _ => return Err(Error::Properties),
    };
    let facing = parse_facing(state.properties.get("facing"))?;
    let (left, width, lock_x, lock_width) = match part {
        ChestPart::Single => (1.0, 14.0, 7.0, 2.0),
        ChestPart::Left => (0.0, 15.0, 0.0, 1.0),
        ChestPart::Right => (1.0, 15.0, 15.0, 1.0),
    };
    let bottom = cube([0, 19], [left, 0.0, 1.0], [width, 10.0, 14.0]);
    let mut lid = cube([0, 0], [left, 0.0, 0.0], [width, 5.0, 14.0]);
    lid.pose_translation = [0.0, 9.0, 1.0];
    let mut lock = cube([0, 0], [lock_x, -2.0, 14.0], [lock_width, 4.0, 1.0]);
    lock.pose_translation = [0.0, 9.0, 1.0];
    let atlas = match part {
        ChestPart::Single => "minecraft:entity/chest/normal",
        ChestPart::Left => "minecraft:entity/chest/normal_left",
        ChestPart::Right => "minecraft:entity/chest/normal_right",
    };
    Ok(Some(Recipe {
        kind: BuiltinKind::Chest(part),
        facing,
        atlas: ResourceId::parse(atlas)?,
        atlas_size: [64, 64],
        cubes: [bottom, lid, lock],
        transform: Transform::Chest {
            facing_degrees: -facing_degrees(facing),
            openness: 0.0,
        },
    }))
}

pub fn recipe(state: &BlockState) -> Result<Option<Recipe>, Error> {
    if let Some(recipe) = bed(state)? {
        return Ok(Some(recipe));
    }
    chest(state)
}

/// BellRenderer's moving body is additive to the nonempty JSON stand. The
/// zero-ringing pose is the only saved-state-independent pose: runtime ticks,
/// shaking and hit direction have no authoritative value in this snapshot.
pub fn bell_body(state: &BlockState) -> Result<Option<BellBody>, Error> {
    if state.name != "minecraft:bell" {
        return Ok(None);
    }
    if state.properties.len() != 3
        || !matches!(
            state.properties.get("attachment").map(String::as_str),
            Some("floor" | "ceiling" | "single_wall" | "double_wall")
        )
        || !matches!(
            state.properties.get("powered").map(String::as_str),
            Some("true" | "false")
        )
    {
        return Err(Error::Properties);
    }
    parse_facing(state.properties.get("facing"))?;
    let mut body = cube([0, 0], [-3.0, -6.0, -3.0], [6.0, 7.0, 6.0]);
    body.pose_translation = [8.0, 12.0, 8.0];
    let base = cube([0, 13], [4.0, 4.0, 4.0], [8.0, 2.0, 8.0]);
    // The child PartPose(-8,-12,-8) composes with its parent's (+8,+12,+8)
    // before cube vertices are transformed, so the static net offset is zero.
    Ok(Some(BellBody {
        atlas: ResourceId::parse("minecraft:entity/bell/bell_body")?,
        atlas_size: [32, 32],
        cubes: [body, base],
    }))
}

fn rotate_x(value: [f64; 3], angle: f64) -> [f64; 3] {
    let (sin, cos) = angle.sin_cos();
    [
        value[0],
        value[1] * cos - value[2] * sin,
        value[1] * sin + value[2] * cos,
    ]
}

fn rotate_y(value: [f64; 3], angle: f64) -> [f64; 3] {
    let (sin, cos) = angle.sin_cos();
    [
        value[0] * cos + value[2] * sin,
        value[1],
        -value[0] * sin + value[2] * cos,
    ]
}

fn rotate_z(value: [f64; 3], angle: f64) -> [f64; 3] {
    let (sin, cos) = angle.sin_cos();
    [
        value[0] * cos - value[1] * sin,
        value[0] * sin + value[1] * cos,
        value[2],
    ]
}

fn transform_point(transform: Transform, cube: &Cube, point: [f64; 3], normal: bool) -> [f64; 3] {
    // ModelPart.translateAndRotate: PartPose offset/16 followed by JOML
    // rotationZYX(z,y,x). Cube vertices are divided by 16 in compile.
    let mut value = rotate_x(point, f64::from(cube.pose_rotation[0]));
    value = rotate_y(value, f64::from(cube.pose_rotation[1]));
    value = rotate_z(value, f64::from(cube.pose_rotation[2]));
    if !normal {
        for (axis, coordinate) in value.iter_mut().enumerate() {
            *coordinate += f64::from(cube.pose_translation[axis]) / 16.0;
        }
    }
    match transform {
        Transform::Bed { facing_degrees } => {
            if !normal {
                for coordinate in &mut value {
                    *coordinate -= 0.5;
                }
            }
            value = rotate_z(value, f64::from(facing_degrees).to_radians());
            if !normal {
                for coordinate in &mut value {
                    *coordinate += 0.5;
                }
            }
            value = rotate_x(value, std::f64::consts::FRAC_PI_2);
            if !normal {
                value[1] += 0.5625;
            }
        }
        Transform::Chest { facing_degrees, .. } => {
            if !normal {
                for coordinate in &mut value {
                    *coordinate -= 0.5;
                }
            }
            value = rotate_y(value, f64::from(facing_degrees).to_radians());
            if !normal {
                for coordinate in &mut value {
                    *coordinate += 0.5;
                }
            }
        }
        Transform::Bell => {}
    }
    [value[0], value[2], value[1]]
}

/// Source-structured ModelPart$Cube/$Polygon geometry, including the native
/// six face vertex orders and atlas rectangles. Floating-point parity with
/// JOML's f32 quaternion/matrix implementation is a separate pixel gate.
/// The caller must still load the pinned atlas and bind these faces to the
/// scene's shared bank, budget and owner namespace.
pub fn bake_faces(recipe: &Recipe) -> Vec<BakedFace> {
    bake_parts(&recipe.cubes, recipe.atlas_size, recipe.transform)
}

pub fn bake_bell_faces(body: &BellBody) -> Vec<BakedFace> {
    bake_parts(&body.cubes, body.atlas_size, Transform::Bell)
}

fn bake_parts(cubes: &[Cube], atlas_size: [u16; 2], transform: Transform) -> Vec<BakedFace> {
    let mut faces = Vec::with_capacity(cubes.len() * 6);
    for (part, cube) in cubes.iter().enumerate() {
        let [x0, y0, z0] = cube.origin.map(|value| f64::from(value) / 16.0);
        let [sx, sy, sz] = cube.size.map(|value| f64::from(value) / 16.0);
        let (x1, y1, z1) = (x0 + sx, y0 + sy, z0 + sz);
        let vertices = [
            [x0, y0, z0],
            [x1, y0, z0],
            [x1, y1, z0],
            [x0, y1, z0],
            [x0, y0, z1],
            [x1, y0, z1],
            [x1, y1, z1],
            [x0, y1, z1],
        ];
        let u0 = f32::from(cube.uv_origin[0]);
        let u1 = u0 + cube.size[2];
        let u2 = u1 + cube.size[0];
        let u3 = u2 + cube.size[0];
        let u4 = u2 + cube.size[2];
        let u5 = u4 + cube.size[0];
        let v0 = f32::from(cube.uv_origin[1]);
        let v1 = v0 + cube.size[2];
        let v2 = v1 + cube.size[1];
        // Java `ModelPart$Cube.<init>` installs polygons in this exact order:
        // east, west, down, up, north, south. Tuple rectangle is (u0,v0,u1,v1).
        for (face, (corners, rectangle, normal)) in [
            ([5, 1, 2, 6], [u2, v1, u4, v2], [1.0, 0.0, 0.0]),
            ([0, 4, 7, 3], [u0, v1, u1, v2], [-1.0, 0.0, 0.0]),
            ([5, 4, 0, 1], [u1, v0, u2, v1], [0.0, -1.0, 0.0]),
            ([2, 3, 7, 6], [u2, v1, u3, v0], [0.0, 1.0, 0.0]),
            ([1, 0, 3, 2], [u1, v1, u2, v2], [0.0, 0.0, -1.0]),
            ([4, 5, 6, 7], [u4, v1, u5, v2], [0.0, 0.0, 1.0]),
        ]
        .into_iter()
        .enumerate()
        {
            let [left, top, right, bottom] = rectangle;
            let atlas = [f32::from(atlas_size[0]), f32::from(atlas_size[1])];
            let uv = [
                [right / atlas[0], top / atlas[1]],
                [left / atlas[0], top / atlas[1]],
                [left / atlas[0], bottom / atlas[1]],
                [right / atlas[0], bottom / atlas[1]],
            ];
            let transformed = transform_point(transform, cube, normal, true);
            faces.push(BakedFace {
                points: corners
                    .map(|index| transform_point(transform, cube, vertices[index], false)),
                uv,
                normal: transformed.map(|value| value as f32),
                part: part as u16,
                face: face as u16,
            });
        }
    }
    faces
}

#[cfg(test)]
#[path = "native_builtin_tests.rs"]
mod tests;
