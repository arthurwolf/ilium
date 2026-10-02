//! Software isometric face rasterizer. Interpolated camera depth makes face
//! visibility independent of chunk submission order (including raised bridges).
#[derive(Clone, Copy)]
pub struct Vertex {
    x: f32,
    y: f32,
    depth: f32,
    u: f32,
    v: f32,
}
impl Vertex {
    pub fn new(x: f32, y: f32, depth: f32, u: f32, v: f32) -> Self {
        Self { x, y, depth, u, v }
    }
}

pub struct Canvas {
    width: usize,
    height: usize,
    pub colors: Vec<[u8; 3]>,
    pub depth: Vec<f32>,
    pub block_owners: Vec<Option<[i32; 3]>>,
    active_block: Option<[i32; 3]>,
    owners: Vec<u64>,
    active_owner: u64,
}
impl Canvas {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            colors: vec![[0; 3]; width * height],
            depth: vec![f32::NEG_INFINITY; width * height],
            block_owners: vec![None; width * height],
            active_block: None,
            owners: vec![u64::MAX; width * height],
            active_owner: 0,
        }
    }
    pub fn face(&mut self, vertices: [Vertex; 4], texture: impl Fn(f32, f32) -> [u8; 3]) {
        self.triangle([vertices[0], vertices[1], vertices[2]], &texture);
        self.triangle([vertices[0], vertices[2], vertices[3]], &texture);
    }
    pub fn face_owned(
        &mut self,
        vertices: [Vertex; 4],
        owner: u64,
        texture: impl Fn(f32, f32) -> [u8; 3],
    ) {
        let previous_owner = self.active_owner;
        self.active_owner = owner;
        self.face(vertices, texture);
        self.active_owner = previous_owner;
    }
    fn triangle(&mut self, vertices: [Vertex; 3], texture: &impl Fn(f32, f32) -> [u8; 3]) {
        let [a, b, c] = vertices;
        let area = edge(a, b, c.x, c.y);
        if area.abs() < 0.00001 || self.width == 0 || self.height == 0 {
            return;
        }
        let left = a.x.min(b.x).min(c.x).floor().max(0.) as usize;
        let right = (a.x.max(b.x).max(c.x).ceil().max(0.) as usize).min(self.width);
        let top = a.y.min(b.y).min(c.y).floor().max(0.) as usize;
        let bottom = (a.y.max(b.y).max(c.y).ceil().max(0.) as usize).min(self.height);
        for y in top..bottom {
            for x in left..right {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                let wa = edge(b, c, px, py) / area;
                let wb = edge(c, a, px, py) / area;
                let wc = 1.0 - wa - wb;
                if wa < -0.00001 || wb < -0.00001 || wc < -0.00001 {
                    continue;
                }
                let depth = wa * a.depth + wb * b.depth + wc * c.depth;
                let index = y * self.width + x;
                // Quantization defines a total rank; pairwise epsilon ties
                // are non-transitive and therefore depend on draw order.
                let rank = (f64::from(depth) * 100_000.0).floor() as i64;
                let previous_rank = (f64::from(self.depth[index]) * 100_000.0).floor() as i64;
                if rank < previous_rank
                    || (rank == previous_rank && self.active_owner >= self.owners[index])
                {
                    continue;
                }
                let u = wa * a.u + wb * b.u + wc * c.u;
                let v = wa * a.v + wb * b.v + wc * c.v;
                self.depth[index] = depth;
                self.owners[index] = self.active_owner;
                self.block_owners[index] = self.active_block;
                self.colors[index] = texture(u.clamp(0., 1.), v.clamp(0., 1.));
            }
        }
    }
}
fn edge(a: Vertex, b: Vertex, x: f32, y: f32) -> f32 {
    (x - a.x) * (b.y - a.y) - (y - a.y) * (b.x - a.x)
}

/// All three unit axes have the same projected length. Raster dots are square
/// in the existing 2×4 Braille / 1:2 terminal-cell convention.
pub fn project(
    point: [f32; 3],
    camera: [f32; 3],
    scale: f32,
    size: [usize; 2],
    uv: [f32; 2],
) -> Vertex {
    let x = point[0] - camera[0];
    let y = point[1] - camera[1];
    let z = point[2] - camera[2];
    Vertex::new(
        size[0] as f32 * 0.5 + (x - y) * 0.866_025_4 * scale,
        size[1] as f32 * 0.5 + ((x + y) * 0.5 - z) * scale,
        x + y + z,
        uv[0],
        uv[1],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn changing_prepared_window_origin_preserves_exact_pixels_and_global_owners() {
        use super::super::{
            catalog::Material,
            generation::{PreparedWorld, Region},
            world::VisibleBlock,
        };
        for offset in [0, -1_000_000_000, 1_000_000_000] {
            let mesh = |base| PreparedWorld {
                region: Region {
                    minimum: [offset + base, offset + base],
                    maximum: [offset + 350, offset + 350],
                },
                blocks: (77..=83)
                    .flat_map(|x| {
                        (77..=83).map(move |y| VisibleBlock {
                            position: [offset + x, offset + y, 25],
                            material: Material::Grass,
                            faces: [true; 3],
                        })
                    })
                    .collect(),
                instances: Vec::new(),
                biomes: Vec::new(),
                cached_chunks: 0,
            };
            let first = mesh(-176);
            let second = mesh(-160);
            for scale in [0.7, 2.8, 11.2] {
                for step in 0..=200 {
                    let fraction = f64::from(step) / 100_000.0;
                    let camera = [
                        f64::from(offset) + 80.0 + fraction,
                        f64::from(offset) + 80.0 + fraction * 0.7,
                        26.0,
                    ];
                    let a = draw_world(&first, camera, scale, [48, 48], 7);
                    let b = draw_world(&second, camera, scale, [48, 48], 7);
                    assert_eq!(
                        a.colors, b.colors,
                        "raster seam offset={offset} scale={scale} step={step}"
                    );
                    assert_eq!(
                        a.block_owners, b.block_owners,
                        "owner seam offset={offset} scale={scale} step={step}"
                    );
                }
            }
        }
    }

    #[test]
    fn nearer_face_wins_in_either_draw_order() {
        let quad = |depth| {
            [
                Vertex::new(1., 1., depth, 0., 0.),
                Vertex::new(7., 1., depth, 1., 0.),
                Vertex::new(7., 7., depth, 1., 1.),
                Vertex::new(1., 7., depth, 0., 1.),
            ]
        };
        let mut first = Canvas::new(8, 8);
        first.face(quad(1.), |_, _| [255, 0, 0]);
        first.face(quad(3.), |_, _| [0, 0, 255]);
        let mut second = Canvas::new(8, 8);
        second.face(quad(3.), |_, _| [0, 0, 255]);
        second.face(quad(1.), |_, _| [255, 0, 0]);
        assert_eq!(first.colors, second.colors);
        assert_eq!(first.colors[36], [0, 0, 255]);
    }
    #[test]
    fn chained_close_depths_and_owned_coplanar_faces_have_total_order() {
        let quad = |depth| {
            [
                Vertex::new(0., 0., depth, 0., 0.),
                Vertex::new(4., 0., depth, 1., 0.),
                Vertex::new(4., 4., depth, 1., 1.),
                Vertex::new(0., 4., depth, 0., 1.),
            ]
        };
        for planes in [
            [
                (0., 0, [255, 0, 0]),
                (0.000007, 1, [0, 255, 0]),
                (0.000014, 2, [0, 0, 255]),
            ],
            [
                (1., 2, [255, 0, 0]),
                (1., 1, [0, 255, 0]),
                (1., 3, [0, 0, 255]),
            ],
        ] {
            let mut first = Canvas::new(4, 4);
            let mut second = Canvas::new(4, 4);
            for (depth, owner, color) in planes {
                first.face_owned(quad(depth), owner, |_, _| color);
            }
            for (depth, owner, color) in planes.into_iter().rev() {
                second.face_owned(quad(depth), owner, |_, _| color);
            }
            assert_eq!(first.colors, second.colors);
        }
    }
    #[test]
    fn axes_have_equal_length_and_empty_clipping_is_safe() {
        let origin = project([0.; 3], [0.; 3], 8., [10, 10], [0.; 2]);
        for axis in [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]] {
            let p = project(axis, [0.; 3], 8., [10, 10], [0.; 2]);
            assert!(((p.x - origin.x).hypot(p.y - origin.y) - 8.).abs() < 0.00001);
        }
        let mut canvas = Canvas::new(0, 0);
        canvas.face([Vertex::new(-1., -1., 1., 0., 0.); 4], |_, _| [255; 3]);
        assert!(canvas.colors.is_empty());
    }
}

/// Visible cube faces in top, west (y+1), east (x+1) order. The caller hides
/// faces touching opaque neighbor blocks. The texture receives face-local UV.
pub fn cube(
    canvas: &mut Canvas,
    origin: [i32; 3],
    camera: [f32; 3],
    scale: f32,
    faces: [bool; 3],
    texture: impl Fn(usize, f32, f32) -> [u8; 3],
) {
    cube_instance(
        canvas,
        Cube {
            origin,
            identity: origin,
            faces,
        },
        camera,
        scale,
        texture,
    );
}
#[derive(Debug, Clone, Copy)]
pub struct Cube {
    pub origin: [i32; 3],
    pub identity: [i32; 3],
    pub faces: [bool; 3],
}
pub fn cube_instance(
    canvas: &mut Canvas,
    instance: Cube,
    camera: [f32; 3],
    scale: f32,
    texture: impl Fn(usize, f32, f32) -> [u8; 3],
) {
    let origin = instance.origin;
    let faces = instance.faces;
    let previous_block = canvas.active_block;
    canvas.active_block = Some(instance.identity);
    let [x, y, z] = origin.map(|coordinate| coordinate as f32);
    let quads = [
        [
            [x, y, z + 1.],
            [x + 1., y, z + 1.],
            [x + 1., y + 1., z + 1.],
            [x, y + 1., z + 1.],
        ],
        [
            [x, y + 1., z + 1.],
            [x + 1., y + 1., z + 1.],
            [x + 1., y + 1., z],
            [x, y + 1., z],
        ],
        [
            [x + 1., y + 1., z + 1.],
            [x + 1., y, z + 1.],
            [x + 1., y, z],
            [x + 1., y + 1., z],
        ],
    ];
    let uv = [[0., 0.], [1., 0.], [1., 1.], [0., 1.]];
    for (face, points) in quads.into_iter().enumerate() {
        if !faces[face] {
            continue;
        }
        let vertices = std::array::from_fn(|index| {
            project(
                points[index],
                camera,
                scale,
                [canvas.width, canvas.height],
                uv[index],
            )
        });
        let shade = [1.0, 0.78, 0.58][face];
        let owner = super::noise::hash2(
            instance.identity[2] as u64,
            i64::from(instance.identity[0]),
            i64::from(instance.identity[1]),
        )
        .wrapping_add(face as u64);
        canvas.face_owned(vertices, owner, |u, v| {
            texture(face, u, v).map(|channel| (f32::from(channel) * shade) as u8)
        });
    }
    canvas.active_block = previous_block;
}

/// Subtract the integer camera origin before floating-point projection.
/// The origin is independent of prepared-window boundaries, preserving exact
/// pixels when a moving camera swaps meshes at a positive or negative seam.
pub fn draw_world(
    world: &super::generation::PreparedWorld,
    camera: [f64; 3],
    scale: f32,
    size: [usize; 2],
    seed: u64,
) -> Canvas {
    let mut canvas = Canvas::new(size[0], size[1]);
    if !scale.is_finite() || scale <= 0.0 || camera.iter().any(|coordinate| !coordinate.is_finite())
    {
        return canvas;
    }
    let base = [camera[0].floor() as i32, camera[1].floor() as i32];
    let local_camera = [
        (camera[0] - f64::from(base[0])) as f32,
        (camera[1] - f64::from(base[1])) as f32,
        camera[2] as f32,
    ];
    for block in &world.blocks {
        let [x, y, z] = block.position;
        let (Some(local_x), Some(local_y)) = (x.checked_sub(base[0]), y.checked_sub(base[1]))
        else {
            continue;
        };
        let origin = [local_x, local_y, z];
        let texture_seed = super::noise::hash2(seed, i64::from(x), i64::from(y));
        cube_instance(
            &mut canvas,
            Cube {
                origin,
                identity: block.position,
                faces: block.faces,
            },
            local_camera,
            scale,
            |face, u, v| {
                block.material.texture(
                    (u * 7.999) as u8,
                    (v * 7.999) as u8,
                    [
                        super::catalog::Face::Top,
                        super::catalog::Face::Left,
                        super::catalog::Face::Right,
                    ][face],
                    texture_seed,
                )
            },
        );
    }
    canvas
}
