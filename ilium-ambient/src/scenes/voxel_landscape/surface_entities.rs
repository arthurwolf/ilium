//! Original authored surface silhouettes, independent of terrain, textures and rasterization.
//!
//! Coordinates are [x, y, z], Z up, forward -Y; one unit is one world block.
//! Geometry is authored here, not copied from the reference geometry. UV rectangles are
//! factual layout data from Mojang's published Bedrock samples. They do not establish Java
//! compatibility: callers MUST inspect `UvStatus` before applying a selected pack's pixels.
//! Sources: https://github.com/Mojang/bedrock-samples/tree/main/resource_pack/models/entity
//! Frozen evidence: ~/dev/.ilium-voxel-expansion/entity-model-candidate/evidence/uv-source-facts.json
//! Nominal coordinates scale against the actual PNG atlas, never pack block resolution.

macro_rules! species {
    ($($variant:ident => $id:literal),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Species { $($variant),+ }
        pub const ALL_SPECIES: &[Species] = &[$(Species::$variant),+];
        impl Species {
            pub const fn id(self) -> &'static str { match self { $(Self::$variant => $id),+ } }
            pub fn from_id(id: &str) -> Option<Self> { ALL_SPECIES.iter().copied().find(|s|s.id()==id) }
        }
    }
}
species! {
 Armadillo=>"minecraft:armadillo", Bogged=>"minecraft:bogged", Camel=>"minecraft:camel",
 Chicken=>"minecraft:chicken", Cow=>"minecraft:cow", Creeper=>"minecraft:creeper",
 Donkey=>"minecraft:donkey", Drowned=>"minecraft:drowned", Enderman=>"minecraft:enderman",
 Fox=>"minecraft:fox", Frog=>"minecraft:frog", Goat=>"minecraft:goat", Horse=>"minecraft:horse",
 Husk=>"minecraft:husk", Llama=>"minecraft:llama", Mooshroom=>"minecraft:mooshroom",
 Ocelot=>"minecraft:ocelot", Panda=>"minecraft:panda", Parched=>"minecraft:parched",
 Parrot=>"minecraft:parrot", Pig=>"minecraft:pig", PolarBear=>"minecraft:polar_bear",
 Rabbit=>"minecraft:rabbit", Sheep=>"minecraft:sheep", Skeleton=>"minecraft:skeleton",
 Slime=>"minecraft:slime", Spider=>"minecraft:spider", Stray=>"minecraft:stray",
 Turtle=>"minecraft:turtle", Witch=>"minecraft:witch", Wolf=>"minecraft:wolf",
 Zombie=>"minecraft:zombie", ZombieHorse=>"minecraft:zombie_horse",
 ZombieVillager=>"minecraft:zombie_villager", Allay=>"minecraft:allay", Cat=>"minecraft:cat",
 IronGolem=>"minecraft:iron_golem", Villager=>"minecraft:villager", Bee=>"minecraft:bee",
 Creaking=>"minecraft:creaking", Pillager=>"minecraft:pillager",
 WanderingTrader=>"minecraft:wandering_trader", TraderLlama=>"minecraft:trader_llama",
 Phantom=>"minecraft:phantom", SkeletonHorse=>"minecraft:skeleton_horse",
 Ravager=>"minecraft:ravager", Vindicator=>"minecraft:vindicator", Evoker=>"minecraft:evoker",
 Vex=>"minecraft:vex", ZombifiedPiglin=>"minecraft:zombified_piglin",
}
/// Changes atlas *layout*, independently of a skin or its actual resolution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AtlasLayout {
    Legacy,
    Modern,
    Bedrock,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClimateSkin {
    Temperate,
    Warm,
    Cold,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Cuboid,
    /// Exactly one zero-thickness axis; render both sides.
    Plane,
}
/// Face array order is Top, Bottom, Front(-Y), Back(+Y), Left(-X), Right(+X).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UvRect {
    pub min: [f32; 2],
    pub max: [f32; 2],
    pub flip_u: bool,
    pub flip_v: bool,
}
impl UvRect {
    const fn from_signed(origin: [f32; 2], size: [f32; 2], mirror: bool) -> Self {
        Self {
            min: [
                if size[0] < 0.0 {
                    origin[0] + size[0]
                } else {
                    origin[0]
                },
                if size[1] < 0.0 {
                    origin[1] + size[1]
                } else {
                    origin[1]
                },
            ],
            max: [
                if size[0] < 0.0 {
                    origin[0]
                } else {
                    origin[0] + size[0]
                },
                if size[1] < 0.0 {
                    origin[1]
                } else {
                    origin[1] + size[1]
                },
            ],
            flip_u: (size[0] < 0.0) ^ mirror,
            flip_v: size[1] < 0.0,
        }
    }
    pub fn normalized(self, nominal: [u16; 2]) -> Option<Self> {
        if nominal.contains(&0) {
            return None;
        }
        let dims = [nominal[0] as f32, nominal[1] as f32];
        if (0..2).any(|i| {
            !self.min[i].is_finite()
                || !self.max[i].is_finite()
                || self.min[i] < 0.0
                || self.max[i] < self.min[i]
                || self.max[i] > dims[i]
        }) {
            return None;
        }
        Some(Self {
            min: [self.min[0] / dims[0], self.min[1] / dims[1]],
            max: [self.max[0] / dims[0], self.max[1] / dims[1]],
            ..self
        })
    }
    /// Actual PNG pixels obtained by scaling normalized atlas coordinates.
    pub fn pixels(self, nominal: [u16; 2], actual: [u32; 2]) -> Option<Self> {
        if actual.contains(&0) {
            return None;
        }
        let uv = self.normalized(nominal)?;
        Some(Self {
            min: [uv.min[0] * actual[0] as f32, uv.min[1] * actual[1] as f32],
            max: [uv.max[0] * actual[0] as f32, uv.max[1] * actual[1] as f32],
            ..uv
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UvStatus {
    /// Factual reference is Bedrock; selected Java pack compatibility still requires verification.
    BedrockReferenceJavaUnverified,
    /// A published Bedrock layout was selected; custom authored assembly may stretch its regions.
    BedrockReference,
    /// No sourced matching part/layout exists. Do not silently sample a guessed atlas rectangle.
    UnsupportedPart,
    /// Explicitly authored reuse of a recorded anatomical source region; not native UV.
    AuthoredCompatibility,
}
#[derive(Clone, Copy, Debug)]
pub struct PartUv {
    pub mapping_note: &'static str,
    pub nominal: [u16; 2],
    pub faces: [UvRect; 6],
    pub source_file: &'static str,
    pub source_bone: &'static str,
    pub source_cube: u16,
    pub status: UvStatus,
}
#[derive(Clone, Debug)]
pub struct Part {
    pub name: &'static str,
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub pivot: [f32; 3],
    /// Euler XYZ angles in radians, applied about pivot, then translation (all local).
    pub rotation: [f32; 3],
    pub shape: Shape,
    pub uv: Option<PartUv>,
    pub texture_semantic: &'static str,
}
impl Part {
    pub fn uv_status(&self) -> UvStatus {
        self.uv.map_or(UvStatus::UnsupportedPart, |u| u.status)
    }
    pub fn corners(&self) -> [[f32; 3]; 8] {
        std::array::from_fn(|index| {
            let mut p: [f32; 3] = std::array::from_fn(|axis| {
                if index & (1 << axis) == 0 {
                    self.min[axis] - self.pivot[axis]
                } else {
                    self.max[axis] - self.pivot[axis]
                }
            });
            for axis in 0..3 {
                let a = (axis + 1) % 3;
                let b = (axis + 2) % 3;
                let (s, c) = self.rotation[axis].sin_cos();
                let (pa, pb) = (p[a], p[b]);
                p[a] = pa * c - pb * s;
                p[b] = pa * s + pb * c;
            }
            std::array::from_fn(|axis| p[axis] + self.pivot[axis])
        })
    }
}
#[derive(Clone, Debug)]
pub struct Model {
    pub atlas_evidence: Vec<AtlasEvidence>,
    pub species: Species,
    pub layout: AtlasLayout,
    pub climate: ClimateSkin,
    pub parts: Vec<Part>,
    /// A semantic species/variant request, not an assumed atlas filename.
    pub texture_semantic: &'static str,
}
impl Model {
    pub fn bounds(&self) -> Option<([f32; 3], [f32; 3])> {
        if self.parts.is_empty() {
            return None;
        }
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for p in &self.parts {
            for corner in p.corners() {
                for i in 0..3 {
                    min[i] = min[i].min(corner[i]);
                    max[i] = max[i].max(corner[i]);
                }
            }
        }
        Some((min, max))
    }
    pub fn unsupported_parts(&self) -> usize {
        self.parts.iter().filter(|p| p.uv.is_none()).count()
    }
}
struct UvReference {
    source: &'static str,
    bone: &'static str,
    cube: u16,
    nominal: [u16; 2],
    faces: [UvRect; 6],
}

fn uv_source(species: Species, layout: AtlasLayout, climate: ClimateSkin) -> &'static str {
    use Species::*;
    match species {
        Cow | Mooshroom => {
            if species == Mooshroom {
                "mooshroom"
            } else if layout == AtlasLayout::Legacy {
                "cow_v1.0"
            } else {
                match climate {
                    ClimateSkin::Cold => "cow.cold",
                    ClimateSkin::Warm => "cow.warm",
                    ClimateSkin::Temperate => "cow.v2",
                }
            }
        }
        Pig => {
            if layout == AtlasLayout::Legacy {
                "pig_v1.0"
            } else {
                "pig.v3"
            }
        }
        Chicken => {
            if climate == ClimateSkin::Cold && layout != AtlasLayout::Legacy {
                "chicken.cold"
            } else {
                "chicken"
            }
        }
        Horse | Donkey | ZombieHorse | SkeletonHorse => {
            if layout == AtlasLayout::Legacy {
                "horse_v2"
            } else {
                "horse_v3"
            }
        }
        TraderLlama => "llama",
        Cat => "cat",
        Villager | WanderingTrader => "villager_v2",
        ZombieVillager => "zombie_villager_v2",
        ZombifiedPiglin => "zombie_pigman",
        Armadillo => "armadillo",
        Bogged => "bogged",
        Camel => "camel",
        Creeper => "creeper",
        Drowned => "drowned",
        Enderman => "enderman",
        Fox => "fox",
        Frog => "frog",
        Goat => "goat",
        Husk => "husk",
        Llama => "llama",
        Ocelot => "ocelot",
        Panda => "panda",
        Parched => "parched",
        Parrot => "parrot",
        PolarBear => "polar_bear",
        Rabbit => "rabbit",
        Sheep => "sheep",
        Skeleton => "skeleton",
        Slime => "slime",
        Spider => "spider",
        Stray => "stray",
        Turtle => "turtle",
        Witch => "witch",
        Wolf => "wolf",
        Zombie => "zombie",
        Allay => "allay",
        IronGolem => "iron_golem",
        Bee => "bee",
        Creaking => "creaking",
        Pillager => "pillager",
        Phantom => "phantom",
        Ravager => "ravager",
        Vindicator => "vindicator",
        Evoker => "evoker",
        Vex => "vex",
    }
}
fn skin(species: Species, climate: ClimateSkin) -> &'static str {
    match (species, climate) {
        (Species::Cow, ClimateSkin::Warm) => "minecraft:cow/warm",
        (Species::Cow, ClimateSkin::Cold) => "minecraft:cow/cold",
        (Species::Pig, ClimateSkin::Warm) => "minecraft:pig/warm",
        (Species::Pig, ClimateSkin::Cold) => "minecraft:pig/cold",
        (Species::Chicken, ClimateSkin::Warm) => "minecraft:chicken/warm",
        (Species::Chicken, ClimateSkin::Cold) => "minecraft:chicken/cold",
        _ => species.id(),
    }
}
fn reference(source: &str, role: &str) -> Option<&'static UvReference> {
    let aliases: &[&str] = match role {
        "body" => &["body", "Body", "torso", "Torso", "body0", "cube"],
        "head" => &["head", "Head", "look_at"],
        "muzzle" => &["nose", "snout", "muzzle", "Muzzle", "beak"],
        "ear" => &[
            "leftEar",
            "rightEar",
            "left_ear",
            "right_ear",
            "ear1",
            "ear2",
            "earLeft",
            "earRight",
            "EarL",
            "EarR",
        ],
        "horn" => &[
            "left_horn",
            "right_horn",
            "horn_left",
            "horn_right",
            "horn1",
            "horn2",
        ],
        "leg" => &[
            "leg0",
            "leftLeg",
            "rightLeg",
            "left_leg",
            "right_leg",
            "front_left_leg",
            "left_hind_leg",
            "leg1",
            "LegFL",
            "LegFR",
            "frontLegLeft",
            "frontLegRight",
            "backLegL",
            "backLegR",
            "left_front_leg",
            "right_front_leg",
            "rearFootLeft",
            "rearFootRight",
            "leg_front",
        ],
        "arm" => &[
            "leftArm",
            "rightArm",
            "left_arm",
            "right_arm",
            "arms",
            "arm0",
            "arm1",
        ],
        "wing" => &[
            "wing0",
            "wing1",
            "left_wing",
            "right_wing",
            "leftWing",
            "rightWing",
            "rightwing_bone",
            "leftwing_bone",
        ],
        "tail" => &["tail", "Tail", "tail1"],
        "shell" => &["shell", "body", "Body"],
        "hump" => &["hump"],
        "comb" => &["comb"],
        "beak" => &["beak"],
        "hat" => &["hat", "hat1"],
        "nose" => &["nose"],
        "eye" => &["eye", "left_eye", "right_eye", "eye0", "eye1"],
        "rib" => &["body", "Body"],
        "robe" => &["body", "Body"],
        _ => &[],
    };
    aliases.iter().find_map(|alias| {
        UV_REFERENCES
            .iter()
            .find(|u| u.source == source && u.bone == *alias && u.cube == 0)
    })
}
struct Builder {
    model: Model,
    source: &'static str,
}
impl Builder {
    fn add(&mut self, name: &'static str, role: &str, min: [f32; 3], size: [f32; 3]) {
        let uv = reference(self.source, role).map(|r| PartUv {
            mapping_note: "Published source region on original authored geometry; target layout compatibility not implied",
            nominal: r.nominal,
            faces: r.faces,
            source_file: r.source,
            source_bone: r.bone,
            source_cube: r.cube,
            status: if self.model.layout == AtlasLayout::Bedrock {
                UvStatus::BedrockReference
            } else {
                UvStatus::BedrockReferenceJavaUnverified
            },
        });
        let max = std::array::from_fn(|i| min[i] + size[i]);
        self.model.parts.push(Part {
            name,
            min,
            max,
            pivot: std::array::from_fn(|i| (min[i] + max[i]) * 0.5),
            rotation: [0.0; 3],
            shape: if size.contains(&0.0) {
                Shape::Plane
            } else {
                Shape::Cuboid
            },
            uv,
            texture_semantic: self.model.texture_semantic,
        });
    }
    fn legs(&mut self, width: f32, length: f32, height: f32, thickness: f32) {
        for (name, x, y) in [
            ("leg_front_left", -width / 2.0, -length / 2.0),
            ("leg_front_right", width / 2.0 - thickness, -length / 2.0),
            ("leg_back_left", -width / 2.0, length / 2.0 - thickness),
            (
                "leg_back_right",
                width / 2.0 - thickness,
                length / 2.0 - thickness,
            ),
        ] {
            self.add(name, "leg", [x, y, 0.0], [thickness, thickness, height]);
        }
    }
    fn quadruped(&mut self, width: f32, length: f32, legs: f32, body_height: f32, head: [f32; 3]) {
        self.add(
            "body",
            "body",
            [-width / 2.0, -length / 2.0, legs],
            [width, length, body_height],
        );
        self.legs(width * 0.85, length * 0.85, legs, width * 0.23);
        self.add(
            "head",
            "head",
            [
                -head[0] / 2.0,
                -length / 2.0 - head[1] * 0.65,
                legs + body_height - head[2] * 0.5,
            ],
            head,
        );
    }
    fn raise_head(&mut self, offset: f32) {
        if let Some(head) = self.model.parts.iter_mut().find(|p| p.name == "head") {
            head.min[2] += offset;
            head.max[2] += offset;
            head.pivot[2] += offset;
        }
    }
    fn ears(&mut self, width: f32, height: f32) {
        let Some(head) = self.model.parts.iter().find(|p| p.name == "head") else {
            return;
        };
        // Attach to this assembly's head, rather than a shared world coordinate.
        let z = head.max[2] - 0.02;
        let y = head.min[1] + (head.max[1] - head.min[1]) * 0.35;
        let width = width.min(head.max[0] - head.min[0] + 0.12);
        self.add(
            "ear_left",
            "ear",
            [-width / 2.0, y, z],
            [0.12, 0.14, height],
        );
        self.add(
            "ear_right",
            "ear",
            [width / 2.0 - 0.12, y, z],
            [0.12, 0.14, height],
        );
    }
    fn humanoid(&mut self, height: f32, slender: bool, arms_forward: bool) {
        let leg = height * 0.4;
        let torso = height * 0.35;
        let w = if slender { 0.28 } else { 0.5 };
        self.add("body", "body", [-w / 2.0, -0.13, leg], [w, 0.26, torso]);
        self.add(
            "head",
            "head",
            [-0.24, -0.24, leg + torso],
            [0.48, 0.48, height * 0.25],
        );
        self.add(
            "leg_left",
            "leg",
            [-w / 2.0, -0.11, 0.0],
            [w * 0.4, 0.22, leg],
        );
        self.add(
            "leg_right",
            "leg",
            [w * 0.1, -0.11, 0.0],
            [w * 0.4, 0.22, leg],
        );
        for (name, x) in [("arm_left", -w / 2.0 - 0.16), ("arm_right", w / 2.0)] {
            self.add(
                name,
                "arm",
                [
                    x,
                    if arms_forward { -0.65 } else { -0.10 },
                    if arms_forward {
                        leg + torso * 0.65
                    } else {
                        leg * 0.7
                    },
                ],
                [
                    0.16,
                    if arms_forward { 0.6 } else { 0.2 },
                    if arms_forward {
                        0.16
                    } else {
                        torso + leg * 0.3
                    },
                ],
            );
        }
    }
}
/// Static pose with specific silhouettes. No texture-file availability is assumed.
pub fn model(species: Species, layout: AtlasLayout, climate: ClimateSkin) -> Model {
    use Species::*;
    let mut b = Builder {
        source: uv_source(species, layout, climate),
        model: Model {
            species,
            layout,
            climate,
            parts: vec![],
            atlas_evidence: vec![],
            texture_semantic: skin(species, climate),
        },
    };
    match species {
        Cow | Mooshroom => {
            b.quadruped(0.85, 1.25, 0.65, 0.62, [0.52, 0.48, 0.52]);
            b.add("muzzle", "muzzle", [-0.22, -1.06, 0.95], [0.44, 0.20, 0.24]);
            for (name, x) in [("horn_left", -0.34), ("horn_right", 0.26)] {
                b.add(name, "horn", [x, -0.87, 1.55], [0.08, 0.1, 0.26]);
            }
            b.add("tail", "tail", [-0.04, 0.6, 0.78], [0.08, 0.45, 0.08]);
            if species == Mooshroom {
                for (cap, stem, y) in [
                    ("mushroom_front", "mushroom_stem_front", -0.35),
                    ("mushroom_back", "mushroom_stem_back", 0.25),
                ] {
                    b.add(stem, "mushroom", [-0.05, y, 1.27], [0.1, 0.1, 0.22]);
                    b.add(cap, "mushroom", [-0.24, y - 0.18, 1.49], [0.48, 0.4, 0.10]);
                }
            }
            if climate == ClimateSkin::Cold && layout != AtlasLayout::Legacy {
                b.add("fur_skirt", "body", [-0.47, -0.53, 0.5], [0.94, 1.05, 0.18]);
            }
        }
        Pig => {
            b.quadruped(0.72, 1.0, 0.35, 0.5, [0.49, 0.4, 0.45]);
            b.add("snout", "muzzle", [-0.18, -0.9, 0.58], [0.36, 0.13, 0.22]);
            b.ears(0.5, 0.16);
            b.add(
                "curled_tail_base",
                "tail",
                [0.24, 0.47, 0.65],
                [0.1, 0.22, 0.1],
            );
            b.add(
                "curled_tail_tip",
                "tail",
                [0.20, 0.62, 0.65],
                [0.18, 0.08, 0.16],
            );
        }
        Sheep => {
            b.quadruped(0.86, 1.12, 0.42, 0.64, [0.40, 0.43, 0.45]);
            b.add(
                "fleece_back",
                "body",
                [-0.47, -0.38, 1.02],
                [0.94, 0.78, 0.16],
            );
            b.add(
                "bare_muzzle",
                "muzzle",
                [-0.15, -0.93, 0.65],
                [0.3, 0.14, 0.22],
            );
        }
        Goat => {
            b.quadruped(0.58, 0.94, 0.7, 0.48, [0.39, 0.42, 0.4]);
            b.add("beard", "muzzle", [-0.08, -0.81, 0.80], [0.16, 0.16, 0.30]);
            for (name, x) in [("horn_left", -0.17), ("horn_right", 0.10)] {
                b.add(name, "horn", [x, -0.67, 1.4], [0.07, 0.20, 0.34]);
            }
            b.ears(0.64, 0.10);
        }
        Horse | Donkey | ZombieHorse | SkeletonHorse => {
            b.quadruped(0.74, 1.40, 1.0, 0.68, [0.36, 0.60, 0.47]);
            b.add("neck", "body", [-0.22, -0.87, 1.45], [0.44, 0.44, 0.65]);
            b.raise_head(0.40);
            b.add(
                "long_muzzle",
                "muzzle",
                [-0.19, -1.28, 1.95],
                [0.38, 0.35, 0.35],
            );
            b.add("mane", "body", [-0.055, -0.67, 1.64], [0.11, 0.51, 0.65]);
            b.ears(0.38, if species == Donkey { 0.43 } else { 0.21 });
            b.add("tail", "tail", [-0.12, 0.61, 0.83], [0.24, 0.57, 0.55]);
            if species == SkeletonHorse {
                b.add(
                    "exposed_spine",
                    "body",
                    [-0.06, -0.4, 1.68],
                    [0.12, 0.9, 0.1],
                );
            }
        }
        Camel => {
            b.quadruped(0.9, 1.75, 1.28, 0.60, [0.43, 0.68, 0.42]);
            b.add("neck", "body", [-0.2, -1.03, 1.64], [0.4, 0.42, 0.7]);
            b.raise_head(0.49);
            b.add("hump", "hump", [-0.33, -0.12, 1.85], [0.66, 0.78, 0.55]);
            b.ears(0.46, 0.18);
            b.add("tail", "tail", [-0.06, 0.83, 1.24], [0.12, 0.48, 0.12]);
        }
        Llama | TraderLlama => {
            b.quadruped(0.72, 1.17, 0.98, 0.58, [0.32, 0.42, 0.43]);
            b.add("neck", "body", [-0.18, -0.73, 1.35], [0.36, 0.34, 0.95]);
            b.raise_head(0.55);
            b.ears(0.34, 0.39);
            b.add("tail", "tail", [-0.12, 0.51, 1.33], [0.24, 0.29, 0.13]);
            if species == TraderLlama {
                b.add("carpet", "body", [-0.4, -0.10, 1.51], [0.8, 0.65, 0.09]);
            }
        }
        Wolf | Fox | Cat | Ocelot => {
            let feline = matches!(species, Cat | Ocelot);
            b.quadruped(
                if feline { 0.35 } else { 0.48 },
                if species == Fox { 0.84 } else { 0.92 },
                0.33,
                0.34,
                [0.32, 0.34, 0.32],
            );
            b.add(
                "muzzle",
                "muzzle",
                [-0.1, -0.84, 0.45],
                [0.20, if feline { 0.1 } else { 0.25 }, 0.15],
            );
            b.ears(0.34, if species == Fox { 0.28 } else { 0.18 });
            b.add(
                "tail_base",
                "tail",
                [-0.10, 0.41, 0.53],
                [0.20, 0.38, if species == Fox { 0.23 } else { 0.13 }],
            );
            b.add(
                "tail_tip",
                "tail",
                [-0.08, 0.73, 0.54],
                [0.16, 0.35, if species == Fox { 0.24 } else { 0.12 }],
            );
            if feline {
                b.add(
                    "whisker_left",
                    "muzzle",
                    [-0.32, -0.78, 0.53],
                    [0.22, 0.01, 0.0],
                );
                b.add(
                    "whisker_right",
                    "muzzle",
                    [0.1, -0.78, 0.53],
                    [0.22, 0.01, 0.0],
                );
            }
        }
        Panda | PolarBear => {
            b.quadruped(1.02, 1.55, 0.45, 0.8, [0.72, 0.54, 0.58]);
            b.add("muzzle", "muzzle", [-0.25, -1.15, 0.72], [0.5, 0.28, 0.28]);
            b.ears(0.72, 0.16);
            if species == Panda {
                b.add(
                    "rounded_back",
                    "body",
                    [-0.43, -0.23, 1.22],
                    [0.86, 0.8, 0.19],
                );
            } else {
                b.add(
                    "shoulder_ridge",
                    "body",
                    [-0.39, -0.65, 1.21],
                    [0.78, 0.52, 0.12],
                );
            }
        }
        Armadillo => {
            b.quadruped(0.58, 0.72, 0.13, 0.34, [0.27, 0.27, 0.23]);
            b.add("shell", "shell", [-0.34, -0.22, 0.34], [0.68, 0.68, 0.24]);
            b.add("tail", "tail", [-0.045, 0.3, 0.14], [0.09, 0.38, 0.09]);
            b.ears(0.25, 0.14);
        }
        Turtle => {
            b.add("shell", "shell", [-0.63, -0.62, 0.15], [1.26, 1.24, 0.43]);
            b.add("head", "head", [-0.23, -0.96, 0.13], [0.46, 0.44, 0.26]);
            for (name, x, y) in [
                ("flipper_front_left", -0.83, -0.45),
                ("flipper_front_right", 0.55, -0.45),
                ("flipper_back_left", -0.83, 0.33),
                ("flipper_back_right", 0.55, 0.33),
            ] {
                b.add(name, "leg", [x, y, 0.06], [0.28, 0.36, 0.11]);
            }
        }
        Frog => {
            b.add("body", "body", [-0.30, -0.18, 0.13], [0.6, 0.56, 0.30]);
            b.add("head", "head", [-0.34, -0.42, 0.30], [0.68, 0.37, 0.25]);
            for (name, x) in [("eye_left", -0.27), ("eye_right", 0.15)] {
                b.add(name, "eye", [x, -0.30, 0.52], [0.12, 0.14, 0.12]);
            }
            b.legs(0.75, 0.72, 0.16, 0.17);
        }
        Rabbit => {
            b.add("body", "body", [-0.21, -0.1, 0.16], [0.42, 0.48, 0.40]);
            b.add("head", "head", [-0.2, -0.42, 0.42], [0.4, 0.34, 0.31]);
            b.ears(0.34, 0.52);
            b.add(
                "hind_haunch_left",
                "leg",
                [-0.30, 0.13, 0.03],
                [0.22, 0.32, 0.33],
            );
            b.add(
                "hind_haunch_right",
                "leg",
                [0.08, 0.13, 0.03],
                [0.22, 0.32, 0.33],
            );
            b.add(
                "front_foot_left",
                "leg",
                [-0.18, -0.29, 0.0],
                [0.12, 0.3, 0.14],
            );
            b.add(
                "front_foot_right",
                "leg",
                [0.06, -0.29, 0.0],
                [0.12, 0.3, 0.14],
            );
            b.add("tail", "tail", [-0.13, 0.39, 0.35], [0.26, 0.15, 0.25]);
        }
        Chicken | Parrot => {
            let parrot = species == Parrot;
            b.add(
                "body",
                "body",
                [-0.21, -0.17, 0.25],
                [0.42, 0.48, if parrot { 0.6 } else { 0.4 }],
            );
            b.add("head", "head", [-0.17, -0.38, 0.67], [0.34, 0.32, 0.35]);
            b.add("beak", "beak", [-0.1, -0.54, 0.74], [0.2, 0.18, 0.16]);
            b.add("leg_left", "leg", [-0.14, -0.04, 0.0], [0.07, 0.13, 0.25]);
            b.add("leg_right", "leg", [0.07, -0.04, 0.0], [0.07, 0.13, 0.25]);
            b.add("wing_left", "wing", [-0.3, -0.07, 0.34], [0.09, 0.35, 0.33]);
            b.add(
                "wing_right",
                "wing",
                [0.21, -0.07, 0.34],
                [0.09, 0.35, 0.33],
            );
            if parrot {
                b.add("crest", "comb", [-0.05, -0.25, 1.02], [0.1, 0.18, 0.23]);
                b.add("long_tail", "tail", [-0.1, 0.26, 0.1], [0.20, 0.52, 0.13]);
            } else {
                b.add("comb", "comb", [-0.035, -0.28, 1.02], [0.07, 0.20, 0.16]);
                b.add("wattle", "head", [-0.07, -0.45, 0.65], [0.14, 0.09, 0.14]);
            }
        }
        Bee => {
            b.add("body", "body", [-0.25, -0.35, 0.27], [0.5, 0.70, 0.43]);
            b.add("head", "head", [-0.23, -0.50, 0.31], [0.46, 0.16, 0.36]);
            b.add("wing_left", "wing", [-0.75, -0.1, 0.73], [0.57, 0.43, 0.0]);
            b.add("wing_right", "wing", [0.18, -0.1, 0.73], [0.57, 0.43, 0.0]);
            b.add(
                "antenna_left",
                "antenna",
                [-0.15, -0.49, 0.65],
                [0.04, 0.04, 0.22],
            );
            b.add(
                "antenna_right",
                "antenna",
                [0.11, -0.49, 0.65],
                [0.04, 0.04, 0.22],
            );
            for (name, y) in [
                ("legs_front", -0.23),
                ("legs_middle", 0.0),
                ("legs_back", 0.23),
            ] {
                b.add(name, "leg", [-0.2, y, 0.13], [0.4, 0.04, 0.16]);
            }
        }
        Spider => {
            b.add("abdomen", "body", [-0.43, 0.0, 0.28], [0.86, 0.80, 0.47]);
            b.add("thorax", "body", [-0.26, -0.36, 0.27], [0.52, 0.44, 0.36]);
            b.add("head", "head", [-0.32, -0.72, 0.29], [0.64, 0.39, 0.37]);
            for (left, right, y) in [
                ("leg_left_front", "leg_right_front", -0.45),
                ("leg_left_mid_front", "leg_right_mid_front", -0.19),
                ("leg_left_mid_back", "leg_right_mid_back", 0.11),
                ("leg_left_back", "leg_right_back", 0.43),
            ] {
                b.add(left, "leg", [-1.15, y, 0.12], [0.95, 0.075, 0.10]);
                b.add(right, "leg", [0.2, y, 0.12], [0.95, 0.075, 0.10]);
            }
        }
        Creeper => {
            b.add("body", "body", [-0.25, -0.2, 0.40], [0.5, 0.4, 0.85]);
            b.add("head", "head", [-0.38, -0.33, 1.25], [0.76, 0.66, 0.6]);
            b.legs(0.65, 0.62, 0.40, 0.22);
        }
        Slime => {
            b.add("outer_gel", "body", [-0.55, -0.55, 0.0], [1.1, 1.1, 1.1]);
            b.add(
                "inner_core",
                "body",
                [-0.36, -0.34, 0.16],
                [0.72, 0.7, 0.65],
            );
            b.add("eye_left", "eye", [-0.30, -0.59, 0.68], [0.20, 0.04, 0.2]);
            b.add("eye_right", "eye", [0.10, -0.59, 0.68], [0.20, 0.04, 0.2]);
            b.add("mouth", "head", [-0.12, -0.59, 0.41], [0.24, 0.04, 0.13]);
        }
        Enderman => {
            b.humanoid(2.95, true, false);
            b.add("long_jaw", "head", [-0.24, -0.29, 2.24], [0.48, 0.12, 0.15]);
        }
        IronGolem => {
            b.humanoid(2.65, false, false);
            b.add(
                "broad_chest",
                "body",
                [-0.60, -0.23, 1.02],
                [1.2, 0.46, 0.83],
            );
            b.add(
                "heavy_arm_left",
                "arm",
                [-0.88, -0.16, 0.43],
                [0.30, 0.33, 1.42],
            );
            b.add(
                "heavy_arm_right",
                "arm",
                [0.58, -0.16, 0.43],
                [0.30, 0.33, 1.42],
            );
            b.add("nose", "nose", [-0.09, -0.48, 2.0], [0.18, 0.25, 0.34]);
        }
        Creaking => {
            b.humanoid(2.75, true, false);
            b.add(
                "crooked_shoulder",
                "body",
                [-0.48, -0.2, 1.75],
                [0.7, 0.38, 0.32],
            );
            b.add(
                "branch_left",
                "arm",
                [-0.59, -0.12, 1.05],
                [0.12, 0.23, 1.32],
            );
            b.add(
                "branch_right",
                "arm",
                [0.27, -0.10, 0.75],
                [0.14, 0.20, 1.63],
            );
            b.add(
                "bark_crown",
                "head",
                [-0.17, -0.14, 2.73],
                [0.19, 0.28, 0.43],
            );
        }
        Allay | Vex => {
            b.humanoid(if species == Allay { 0.77 } else { 0.90 }, true, false);
            b.add("wing_left", "wing", [-0.61, 0.11, 0.32], [0.4, 0.0, 0.47]);
            b.add("wing_right", "wing", [0.21, 0.11, 0.32], [0.4, 0.0, 0.47]);
            if species == Vex {
                b.add("sword", "weapon", [0.28, -0.22, 0.22], [0.08, 0.1, 0.57]);
            }
        }
        Phantom => {
            b.add("body", "body", [-0.30, -0.47, 0.55], [0.6, 1.0, 0.21]);
            b.add("head", "head", [-0.26, -0.78, 0.48], [0.52, 0.4, 0.33]);
            b.add("wing_left", "wing", [-1.9, -0.24, 0.66], [1.6, 0.86, 0.0]);
            b.add("wing_right", "wing", [0.3, -0.24, 0.66], [1.6, 0.86, 0.0]);
            b.add("tail", "tail", [-0.11, 0.50, 0.51], [0.22, 1.14, 0.13]);
        }
        Ravager => {
            b.quadruped(1.4, 1.75, 0.88, 1.0, [0.91, 0.70, 0.76]);
            b.add("broad_nose", "nose", [-0.4, -1.54, 1.09], [0.8, 0.38, 0.46]);
            for (name, x) in [("horn_left", -0.54), ("horn_right", 0.42)] {
                b.add(name, "horn", [x, -1.04, 1.89], [0.12, 0.46, 0.43]);
            }
            b.add(
                "shoulder_mass",
                "body",
                [-0.64, -0.37, 1.79],
                [1.28, 0.75, 0.25],
            );
        }
        Villager | WanderingTrader | ZombieVillager | Witch | Pillager | Vindicator | Evoker => {
            b.humanoid(1.95, false, species == ZombieVillager);
            b.add("nose", "nose", [-0.09, -0.46, 1.44], [0.18, 0.23, 0.38]);
            if matches!(species, Villager | WanderingTrader | Witch | Evoker) {
                b.add("robe", "robe", [-0.30, -0.17, 0.34], [0.6, 0.38, 0.88]);
            }
            if matches!(species, Villager | WanderingTrader | Evoker) {
                b.add(
                    "folded_arms",
                    "arm",
                    [-0.39, -0.35, 1.15],
                    [0.78, 0.21, 0.18],
                );
            }
            if species == Witch {
                b.add("hat_brim", "hat", [-0.49, -0.41, 1.93], [0.98, 0.83, 0.09]);
                b.add("hat_lower", "hat", [-0.29, -0.23, 2.02], [0.58, 0.48, 0.31]);
                b.add("hat_tip", "hat", [-0.16, -0.12, 2.33], [0.28, 0.25, 0.30]);
            }
            if species == WanderingTrader {
                b.add("hood", "hat", [-0.28, 0.1, 1.65], [0.56, 0.25, 0.34]);
            }
            if species == Pillager {
                b.add(
                    "crossbow_stock",
                    "weapon",
                    [-0.09, -0.73, 1.04],
                    [0.18, 0.55, 0.13],
                );
                b.add(
                    "crossbow_bow",
                    "weapon",
                    [-0.52, -0.69, 1.03],
                    [1.04, 0.10, 0.12],
                );
            }
            if species == Vindicator {
                b.add(
                    "axe_handle",
                    "weapon",
                    [0.38, -0.25, 0.83],
                    [0.06, 0.07, 0.73],
                );
                b.add(
                    "axe_blade",
                    "weapon",
                    [0.29, -0.28, 1.45],
                    [0.38, 0.12, 0.25],
                );
            }
            if species == Evoker {
                b.add("collar", "body", [-0.32, -0.2, 1.4], [0.64, 0.4, 0.14]);
            }
        }
        Skeleton | Stray | Bogged | Parched => {
            b.humanoid(1.92, true, false);
            b.add("rib_cage", "rib", [-0.25, -0.16, 0.92], [0.50, 0.32, 0.35]);
            b.add("jaw", "head", [-0.20, -0.25, 1.46], [0.4, 0.13, 0.12]);
            if species == Stray {
                b.add(
                    "ragged_tunic",
                    "robe",
                    [-0.25, -0.18, 0.7],
                    [0.50, 0.36, 0.3],
                );
            }
            if species == Bogged {
                b.add(
                    "moss_crown",
                    "head",
                    [-0.28, -0.26, 1.93],
                    [0.56, 0.52, 0.10],
                );
                b.add(
                    "mushroom_cap",
                    "mushroom",
                    [0.10, -0.1, 2.08],
                    [0.32, 0.30, 0.10],
                );
                b.add(
                    "mushroom_stem",
                    "mushroom",
                    [0.22, 0.02, 2.0],
                    [0.08, 0.08, 0.10],
                );
            }
            if species == Parched {
                b.add("head_wrap", "head", [-0.26, -0.25, 1.68], [0.52, 0.5, 0.13]);
            }
        }
        Zombie | Husk | Drowned | ZombifiedPiglin => {
            b.humanoid(1.95, false, true);
            if species == Husk {
                b.add(
                    "ragged_collar",
                    "body",
                    [-0.28, -0.15, 1.30],
                    [0.56, 0.32, 0.15],
                );
            }
            if species == Drowned {
                b.add(
                    "seaweed_shoulder",
                    "body",
                    [-0.34, -0.13, 1.16],
                    [0.15, 0.30, 0.4],
                );
            }
            if species == ZombifiedPiglin {
                b.add("snout", "muzzle", [-0.18, -0.41, 1.56], [0.36, 0.18, 0.2]);
                b.ears(0.63, 0.17);
            }
        }
    }
    b.model
}
/// Surface event composition: caller decides habitat and event eligibility.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceEvent {
    SpiderJockey,
    ChickenJockey,
    HorseJockey,
    ChargedCreeper,
    LightningPig,
}
pub fn event_models(event: SurfaceEvent, layout: AtlasLayout) -> Vec<Model> {
    let (mount, rider, offset) = match event {
        SurfaceEvent::SpiderJockey => (Species::Spider, Some(Species::Skeleton), [0.0, 0.2, 0.70]),
        SurfaceEvent::ChickenJockey => (Species::Chicken, Some(Species::Zombie), [0.0, 0.02, 0.6]),
        SurfaceEvent::HorseJockey => (
            Species::SkeletonHorse,
            Some(Species::Skeleton),
            [0.0, 0.0, 1.65],
        ),
        SurfaceEvent::ChargedCreeper => (Species::Creeper, None, [0.0; 3]),
        SurfaceEvent::LightningPig => (Species::ZombifiedPiglin, None, [0.0; 3]),
    };
    let mut models = vec![model(mount, layout, ClimateSkin::Temperate)];
    if let Some(rider) = rider {
        let mut m = model(rider, layout, ClimateSkin::Temperate);
        let scale = if event == SurfaceEvent::ChickenJockey {
            0.5
        } else {
            0.82
        };
        for p in &mut m.parts {
            for (axis, translation) in offset.iter().enumerate() {
                p.min[axis] = p.min[axis] * scale + translation;
                p.max[axis] = p.max[axis] * scale + translation;
                p.pivot[axis] = p.pivot[axis] * scale + translation;
            }
            if p.name.starts_with("leg_") {
                p.pivot[2] = p.max[2];
                p.rotation[0] = -std::f32::consts::FRAC_PI_2;
            }
        }
        models.push(m);
    }
    if event == SurfaceEvent::ChargedCreeper {
        // Keep ordinary skin on the core. The separate translucent aura requires
        // its own validated atlas; no normal-skin UV is falsely relabeled as aura.
        let aura: Vec<_> = models[0]
            .parts
            .iter()
            .cloned()
            .map(|mut p| {
                p.name = "charged_aura";
                p.texture_semantic = "minecraft:creeper/charged";
                p.uv = None;
                for axis in 0..3 {
                    let half = (p.max[axis] - p.min[axis]) * 0.55;
                    p.min[axis] = p.pivot[axis] - half;
                    p.max[axis] = p.pivot[axis] + half;
                }
                p
            })
            .collect();
        models[0].parts.extend(aura);
    }
    models
}
#[rustfmt::skip]
const UV_REFERENCES: &[UvReference] = &[
UvReference { source: "allay", bone: "look_at", cube: 0, nominal: [32,32], faces: [UvRect::from_signed([5.0000,0.0000],[5.0000,5.0000],false),UvRect::from_signed([10.0000,0.0000],[5.0000,5.0000],false),UvRect::from_signed([5.0000,5.0000],[5.0000,5.0000],false),UvRect::from_signed([15.0000,5.0000],[5.0000,5.0000],false),UvRect::from_signed([0.0000,5.0000],[5.0000,5.0000],false),UvRect::from_signed([10.0000,5.0000],[5.0000,5.0000],false)] },
UvReference { source: "allay", bone: "body", cube: 0, nominal: [32,32], faces: [UvRect::from_signed([2.0000,10.0000],[3.0000,2.0000],false),UvRect::from_signed([5.0000,10.0000],[3.0000,2.0000],false),UvRect::from_signed([2.0000,12.0000],[3.0000,4.0000],false),UvRect::from_signed([7.0000,12.0000],[3.0000,4.0000],false),UvRect::from_signed([0.0000,12.0000],[2.0000,4.0000],false),UvRect::from_signed([5.0000,12.0000],[2.0000,4.0000],false)] },
UvReference { source: "allay", bone: "body", cube: 1, nominal: [32,32], faces: [UvRect::from_signed([2.0000,16.0000],[3.0000,2.0000],false),UvRect::from_signed([5.0000,16.0000],[3.0000,2.0000],false),UvRect::from_signed([2.0000,18.0000],[3.0000,5.0000],false),UvRect::from_signed([7.0000,18.0000],[3.0000,5.0000],false),UvRect::from_signed([0.0000,18.0000],[2.0000,5.0000],false),UvRect::from_signed([5.0000,18.0000],[2.0000,5.0000],false)] },
UvReference { source: "allay", bone: "right_arm", cube: 0, nominal: [32,32], faces: [UvRect::from_signed([25.0000,0.0000],[1.0000,2.0000],false),UvRect::from_signed([26.0000,0.0000],[1.0000,2.0000],false),UvRect::from_signed([25.0000,2.0000],[1.0000,4.0000],false),UvRect::from_signed([28.0000,2.0000],[1.0000,4.0000],false),UvRect::from_signed([23.0000,2.0000],[2.0000,4.0000],false),UvRect::from_signed([26.0000,2.0000],[2.0000,4.0000],false)] },
UvReference { source: "allay", bone: "left_arm", cube: 0, nominal: [32,32], faces: [UvRect::from_signed([25.0000,6.0000],[1.0000,2.0000],false),UvRect::from_signed([26.0000,6.0000],[1.0000,2.0000],false),UvRect::from_signed([25.0000,8.0000],[1.0000,4.0000],false),UvRect::from_signed([28.0000,8.0000],[1.0000,4.0000],false),UvRect::from_signed([23.0000,8.0000],[2.0000,4.0000],false),UvRect::from_signed([26.0000,8.0000],[2.0000,4.0000],false)] },
UvReference { source: "allay", bone: "left_wing", cube: 0, nominal: [32,32], faces: [UvRect::from_signed([24.0000,14.0000],[0.0000,8.0000],false),UvRect::from_signed([24.0000,14.0000],[0.0000,8.0000],false),UvRect::from_signed([24.0000,22.0000],[0.0000,5.0000],false),UvRect::from_signed([32.0000,22.0000],[0.0000,5.0000],false),UvRect::from_signed([16.0000,22.0000],[8.0000,5.0000],false),UvRect::from_signed([24.0000,22.0000],[8.0000,5.0000],false)] },
UvReference { source: "allay", bone: "right_wing", cube: 0, nominal: [32,32], faces: [UvRect::from_signed([24.0000,14.0000],[0.0000,8.0000],false),UvRect::from_signed([24.0000,14.0000],[0.0000,8.0000],false),UvRect::from_signed([24.0000,22.0000],[0.0000,5.0000],false),UvRect::from_signed([32.0000,22.0000],[0.0000,5.0000],false),UvRect::from_signed([16.0000,22.0000],[8.0000,5.0000],false),UvRect::from_signed([24.0000,22.0000],[8.0000,5.0000],false)] },
UvReference { source: "armadillo", bone: "body", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([12.0000,20.0000],[8.0000,12.0000],false),UvRect::from_signed([20.0000,20.0000],[8.0000,12.0000],false),UvRect::from_signed([12.0000,32.0000],[8.0000,8.0000],false),UvRect::from_signed([32.0000,32.0000],[8.0000,8.0000],false),UvRect::from_signed([0.0000,32.0000],[12.0000,8.0000],false),UvRect::from_signed([20.0000,32.0000],[12.0000,8.0000],false)] },
UvReference { source: "armadillo", bone: "body", cube: 1, nominal: [64,64], faces: [UvRect::from_signed([12.0000,40.0000],[8.0000,12.0000],false),UvRect::from_signed([20.0000,40.0000],[8.0000,12.0000],false),UvRect::from_signed([12.0000,52.0000],[8.0000,8.0000],false),UvRect::from_signed([32.0000,52.0000],[8.0000,8.0000],false),UvRect::from_signed([0.0000,52.0000],[12.0000,8.0000],false),UvRect::from_signed([20.0000,52.0000],[12.0000,8.0000],false)] },
UvReference { source: "armadillo", bone: "tail", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([45.0000,53.0000],[1.0000,1.0000],false),UvRect::from_signed([46.0000,53.0000],[1.0000,1.0000],false),UvRect::from_signed([45.0000,54.0000],[1.0000,6.0000],false),UvRect::from_signed([47.0000,54.0000],[1.0000,6.0000],false),UvRect::from_signed([44.0000,54.0000],[1.0000,6.0000],false),UvRect::from_signed([46.0000,54.0000],[1.0000,6.0000],false)] },
UvReference { source: "armadillo", bone: "head", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([45.0000,15.0000],[3.0000,2.0000],false),UvRect::from_signed([48.0000,15.0000],[3.0000,2.0000],false),UvRect::from_signed([45.0000,17.0000],[3.0000,5.0000],false),UvRect::from_signed([50.0000,17.0000],[3.0000,5.0000],false),UvRect::from_signed([43.0000,17.0000],[2.0000,5.0000],false),UvRect::from_signed([48.0000,17.0000],[2.0000,5.0000],false)] },
UvReference { source: "armadillo", bone: "right_ear", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([43.0000,10.0000],[2.0000,0.0000],false),UvRect::from_signed([45.0000,10.0000],[2.0000,0.0000],false),UvRect::from_signed([43.0000,10.0000],[2.0000,5.0000],false),UvRect::from_signed([45.0000,10.0000],[2.0000,5.0000],false),UvRect::from_signed([43.0000,10.0000],[0.0000,5.0000],false),UvRect::from_signed([45.0000,10.0000],[0.0000,5.0000],false)] },
UvReference { source: "armadillo", bone: "left_ear", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([47.0000,10.0000],[2.0000,0.0000],false),UvRect::from_signed([49.0000,10.0000],[2.0000,0.0000],false),UvRect::from_signed([47.0000,10.0000],[2.0000,5.0000],false),UvRect::from_signed([49.0000,10.0000],[2.0000,5.0000],false),UvRect::from_signed([47.0000,10.0000],[0.0000,5.0000],false),UvRect::from_signed([49.0000,10.0000],[0.0000,5.0000],false)] },
UvReference { source: "armadillo", bone: "right_hind_leg", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([53.0000,31.0000],[2.0000,2.0000],false),UvRect::from_signed([55.0000,31.0000],[2.0000,2.0000],false),UvRect::from_signed([53.0000,33.0000],[2.0000,3.0000],false),UvRect::from_signed([57.0000,33.0000],[2.0000,3.0000],false),UvRect::from_signed([51.0000,33.0000],[2.0000,3.0000],false),UvRect::from_signed([55.0000,33.0000],[2.0000,3.0000],false)] },
UvReference { source: "armadillo", bone: "left_hind_leg", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([44.0000,31.0000],[2.0000,2.0000],false),UvRect::from_signed([46.0000,31.0000],[2.0000,2.0000],false),UvRect::from_signed([44.0000,33.0000],[2.0000,3.0000],false),UvRect::from_signed([48.0000,33.0000],[2.0000,3.0000],false),UvRect::from_signed([42.0000,33.0000],[2.0000,3.0000],false),UvRect::from_signed([46.0000,33.0000],[2.0000,3.0000],false)] },
UvReference { source: "armadillo", bone: "right_front_leg", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([53.0000,43.0000],[2.0000,2.0000],false),UvRect::from_signed([55.0000,43.0000],[2.0000,2.0000],false),UvRect::from_signed([53.0000,45.0000],[2.0000,3.0000],false),UvRect::from_signed([57.0000,45.0000],[2.0000,3.0000],false),UvRect::from_signed([51.0000,45.0000],[2.0000,3.0000],false),UvRect::from_signed([55.0000,45.0000],[2.0000,3.0000],false)] },
UvReference { source: "armadillo", bone: "left_front_leg", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([44.0000,43.0000],[2.0000,2.0000],false),UvRect::from_signed([46.0000,43.0000],[2.0000,2.0000],false),UvRect::from_signed([44.0000,45.0000],[2.0000,3.0000],false),UvRect::from_signed([48.0000,45.0000],[2.0000,3.0000],false),UvRect::from_signed([42.0000,45.0000],[2.0000,3.0000],false),UvRect::from_signed([46.0000,45.0000],[2.0000,3.0000],false)] },
UvReference { source: "armadillo", bone: "body_rolled_up", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([10.0000,0.0000],[10.0000,10.0000],false),UvRect::from_signed([20.0000,0.0000],[10.0000,10.0000],false),UvRect::from_signed([10.0000,10.0000],[10.0000,10.0000],false),UvRect::from_signed([30.0000,10.0000],[10.0000,10.0000],false),UvRect::from_signed([0.0000,10.0000],[10.0000,10.0000],false),UvRect::from_signed([20.0000,10.0000],[10.0000,10.0000],false)] },
UvReference { source: "bee", bone: "body", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([10.0000,0.0000],[7.0000,10.0000],false),UvRect::from_signed([17.0000,0.0000],[7.0000,10.0000],false),UvRect::from_signed([10.0000,10.0000],[7.0000,7.0000],false),UvRect::from_signed([27.0000,10.0000],[7.0000,7.0000],false),UvRect::from_signed([0.0000,10.0000],[10.0000,7.0000],false),UvRect::from_signed([17.0000,10.0000],[10.0000,7.0000],false)] },
UvReference { source: "bee", bone: "body", cube: 1, nominal: [64,64], faces: [UvRect::from_signed([5.0000,0.0000],[1.0000,3.0000],false),UvRect::from_signed([6.0000,0.0000],[1.0000,3.0000],false),UvRect::from_signed([5.0000,3.0000],[1.0000,2.0000],false),UvRect::from_signed([9.0000,3.0000],[1.0000,2.0000],false),UvRect::from_signed([2.0000,3.0000],[3.0000,2.0000],false),UvRect::from_signed([6.0000,3.0000],[3.0000,2.0000],false)] },
UvReference { source: "bee", bone: "body", cube: 2, nominal: [64,64], faces: [UvRect::from_signed([5.0000,3.0000],[1.0000,3.0000],false),UvRect::from_signed([6.0000,3.0000],[1.0000,3.0000],false),UvRect::from_signed([5.0000,6.0000],[1.0000,2.0000],false),UvRect::from_signed([9.0000,6.0000],[1.0000,2.0000],false),UvRect::from_signed([2.0000,6.0000],[3.0000,2.0000],false),UvRect::from_signed([6.0000,6.0000],[3.0000,2.0000],false)] },
UvReference { source: "bee", bone: "stinger", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([28.0000,7.0000],[0.0000,2.0000],false),UvRect::from_signed([28.0000,7.0000],[0.0000,2.0000],false),UvRect::from_signed([28.0000,9.0000],[0.0000,1.0000],false),UvRect::from_signed([30.0000,9.0000],[0.0000,1.0000],false),UvRect::from_signed([26.0000,9.0000],[2.0000,1.0000],false),UvRect::from_signed([28.0000,9.0000],[2.0000,1.0000],false)] },
UvReference { source: "bee", bone: "rightwing_bone", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([6.0000,18.0000],[9.0000,6.0000],false),UvRect::from_signed([15.0000,18.0000],[9.0000,6.0000],false),UvRect::from_signed([6.0000,24.0000],[9.0000,0.0000],false),UvRect::from_signed([21.0000,24.0000],[9.0000,0.0000],false),UvRect::from_signed([0.0000,24.0000],[6.0000,0.0000],false),UvRect::from_signed([15.0000,24.0000],[6.0000,0.0000],false)] },
UvReference { source: "bee", bone: "leftwing_bone", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([15.0000,24.0000],[9.0000,6.0000],false),UvRect::from_signed([24.0000,24.0000],[9.0000,6.0000],false),UvRect::from_signed([15.0000,30.0000],[9.0000,0.0000],false),UvRect::from_signed([30.0000,30.0000],[9.0000,0.0000],false),UvRect::from_signed([9.0000,30.0000],[6.0000,0.0000],false),UvRect::from_signed([24.0000,30.0000],[6.0000,0.0000],false)] },
UvReference { source: "bee", bone: "leg_front", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([26.0000,1.0000],[7.0000,0.0000],false),UvRect::from_signed([33.0000,1.0000],[7.0000,0.0000],false),UvRect::from_signed([26.0000,1.0000],[7.0000,2.0000],false),UvRect::from_signed([33.0000,1.0000],[7.0000,2.0000],false),UvRect::from_signed([26.0000,1.0000],[0.0000,2.0000],false),UvRect::from_signed([33.0000,1.0000],[0.0000,2.0000],false)] },
UvReference { source: "bee", bone: "leg_mid", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([26.0000,3.0000],[7.0000,0.0000],false),UvRect::from_signed([33.0000,3.0000],[7.0000,0.0000],false),UvRect::from_signed([26.0000,3.0000],[7.0000,2.0000],false),UvRect::from_signed([33.0000,3.0000],[7.0000,2.0000],false),UvRect::from_signed([26.0000,3.0000],[0.0000,2.0000],false),UvRect::from_signed([33.0000,3.0000],[0.0000,2.0000],false)] },
UvReference { source: "bee", bone: "leg_back", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([26.0000,5.0000],[7.0000,0.0000],false),UvRect::from_signed([33.0000,5.0000],[7.0000,0.0000],false),UvRect::from_signed([26.0000,5.0000],[7.0000,2.0000],false),UvRect::from_signed([33.0000,5.0000],[7.0000,2.0000],false),UvRect::from_signed([26.0000,5.0000],[0.0000,2.0000],false),UvRect::from_signed([33.0000,5.0000],[0.0000,2.0000],false)] },
UvReference { source: "bogged", bone: "body", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([20.0000,16.0000],[8.0000,4.0000],false),UvRect::from_signed([28.0000,16.0000],[8.0000,4.0000],false),UvRect::from_signed([20.0000,20.0000],[8.0000,12.0000],false),UvRect::from_signed([32.0000,20.0000],[8.0000,12.0000],false),UvRect::from_signed([16.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([28.0000,20.0000],[4.0000,12.0000],false)] },
UvReference { source: "bogged", bone: "head", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([8.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([16.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([8.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([24.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([0.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([16.0000,8.0000],[8.0000,8.0000],false)] },
UvReference { source: "bogged", bone: "mushrooms", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([50.0000,22.0000],[6.0000,0.0000],false),UvRect::from_signed([56.0000,22.0000],[6.0000,0.0000],false),UvRect::from_signed([50.0000,22.0000],[6.0000,4.0000],false),UvRect::from_signed([56.0000,22.0000],[6.0000,4.0000],false),UvRect::from_signed([50.0000,22.0000],[0.0000,4.0000],false),UvRect::from_signed([56.0000,22.0000],[0.0000,4.0000],false)] },
UvReference { source: "bogged", bone: "mushrooms", cube: 1, nominal: [64,32], faces: [UvRect::from_signed([50.0000,22.0000],[6.0000,0.0000],false),UvRect::from_signed([56.0000,22.0000],[6.0000,0.0000],false),UvRect::from_signed([50.0000,22.0000],[6.0000,4.0000],false),UvRect::from_signed([56.0000,22.0000],[6.0000,4.0000],false),UvRect::from_signed([50.0000,22.0000],[0.0000,4.0000],false),UvRect::from_signed([56.0000,22.0000],[0.0000,4.0000],false)] },
UvReference { source: "bogged", bone: "mushrooms", cube: 2, nominal: [64,32], faces: [UvRect::from_signed([50.0000,16.0000],[6.0000,0.0000],false),UvRect::from_signed([56.0000,16.0000],[6.0000,0.0000],false),UvRect::from_signed([50.0000,16.0000],[6.0000,4.0000],false),UvRect::from_signed([56.0000,16.0000],[6.0000,4.0000],false),UvRect::from_signed([50.0000,16.0000],[0.0000,4.0000],false),UvRect::from_signed([56.0000,16.0000],[0.0000,4.0000],false)] },
UvReference { source: "bogged", bone: "mushrooms", cube: 3, nominal: [64,32], faces: [UvRect::from_signed([50.0000,16.0000],[6.0000,0.0000],false),UvRect::from_signed([56.0000,16.0000],[6.0000,0.0000],false),UvRect::from_signed([50.0000,16.0000],[6.0000,4.0000],false),UvRect::from_signed([56.0000,16.0000],[6.0000,4.0000],false),UvRect::from_signed([50.0000,16.0000],[0.0000,4.0000],false),UvRect::from_signed([56.0000,16.0000],[0.0000,4.0000],false)] },
UvReference { source: "bogged", bone: "mushrooms", cube: 4, nominal: [64,32], faces: [UvRect::from_signed([50.0000,27.0000],[6.0000,0.0000],false),UvRect::from_signed([56.0000,27.0000],[6.0000,0.0000],false),UvRect::from_signed([50.0000,27.0000],[6.0000,5.0000],false),UvRect::from_signed([56.0000,27.0000],[6.0000,5.0000],false),UvRect::from_signed([50.0000,27.0000],[0.0000,5.0000],false),UvRect::from_signed([56.0000,27.0000],[0.0000,5.0000],false)] },
UvReference { source: "bogged", bone: "mushrooms", cube: 5, nominal: [64,32], faces: [UvRect::from_signed([50.0000,27.0000],[6.0000,0.0000],false),UvRect::from_signed([56.0000,27.0000],[6.0000,0.0000],false),UvRect::from_signed([50.0000,27.0000],[6.0000,5.0000],false),UvRect::from_signed([56.0000,27.0000],[6.0000,5.0000],false),UvRect::from_signed([50.0000,27.0000],[0.0000,5.0000],false),UvRect::from_signed([56.0000,27.0000],[0.0000,5.0000],false)] },
UvReference { source: "bogged", bone: "hat", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([40.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([48.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([40.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([56.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([32.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([48.0000,8.0000],[8.0000,8.0000],false)] },
UvReference { source: "bogged", bone: "rightArm", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([42.0000,16.0000],[2.0000,2.0000],false),UvRect::from_signed([44.0000,16.0000],[2.0000,2.0000],false),UvRect::from_signed([42.0000,18.0000],[2.0000,12.0000],false),UvRect::from_signed([46.0000,18.0000],[2.0000,12.0000],false),UvRect::from_signed([40.0000,18.0000],[2.0000,12.0000],false),UvRect::from_signed([44.0000,18.0000],[2.0000,12.0000],false)] },
UvReference { source: "bogged", bone: "leftArm", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([42.0000,16.0000],[2.0000,2.0000],true),UvRect::from_signed([44.0000,16.0000],[2.0000,2.0000],true),UvRect::from_signed([42.0000,18.0000],[2.0000,12.0000],true),UvRect::from_signed([46.0000,18.0000],[2.0000,12.0000],true),UvRect::from_signed([40.0000,18.0000],[2.0000,12.0000],true),UvRect::from_signed([44.0000,18.0000],[2.0000,12.0000],true)] },
UvReference { source: "bogged", bone: "rightLeg", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([2.0000,16.0000],[2.0000,2.0000],false),UvRect::from_signed([4.0000,16.0000],[2.0000,2.0000],false),UvRect::from_signed([2.0000,18.0000],[2.0000,12.0000],false),UvRect::from_signed([6.0000,18.0000],[2.0000,12.0000],false),UvRect::from_signed([0.0000,18.0000],[2.0000,12.0000],false),UvRect::from_signed([4.0000,18.0000],[2.0000,12.0000],false)] },
UvReference { source: "bogged", bone: "leftLeg", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([2.0000,16.0000],[2.0000,2.0000],true),UvRect::from_signed([4.0000,16.0000],[2.0000,2.0000],true),UvRect::from_signed([2.0000,18.0000],[2.0000,12.0000],true),UvRect::from_signed([6.0000,18.0000],[2.0000,12.0000],true),UvRect::from_signed([0.0000,18.0000],[2.0000,12.0000],true),UvRect::from_signed([4.0000,18.0000],[2.0000,12.0000],true)] },
UvReference { source: "camel", bone: "body", cube: 0, nominal: [128,128], faces: [UvRect::from_signed([27.0000,25.0000],[15.0000,27.0000],false),UvRect::from_signed([42.0000,25.0000],[15.0000,27.0000],false),UvRect::from_signed([27.0000,52.0000],[15.0000,12.0000],false),UvRect::from_signed([69.0000,52.0000],[15.0000,12.0000],false),UvRect::from_signed([0.0000,52.0000],[27.0000,12.0000],false),UvRect::from_signed([42.0000,52.0000],[27.0000,12.0000],false)] },
UvReference { source: "camel", bone: "saddle", cube: 0, nominal: [128,128], faces: [UvRect::from_signed([85.0000,64.0000],[9.0000,11.0000],false),UvRect::from_signed([94.0000,64.0000],[9.0000,11.0000],false),UvRect::from_signed([85.0000,75.0000],[9.0000,5.0000],false),UvRect::from_signed([105.0000,75.0000],[9.0000,5.0000],false),UvRect::from_signed([74.0000,75.0000],[11.0000,5.0000],false),UvRect::from_signed([94.0000,75.0000],[11.0000,5.0000],false)] },
UvReference { source: "camel", bone: "saddle", cube: 1, nominal: [128,128], faces: [UvRect::from_signed([103.0000,114.0000],[7.0000,11.0000],false),UvRect::from_signed([110.0000,114.0000],[7.0000,11.0000],false),UvRect::from_signed([103.0000,125.0000],[7.0000,3.0000],false),UvRect::from_signed([121.0000,125.0000],[7.0000,3.0000],false),UvRect::from_signed([92.0000,125.0000],[11.0000,3.0000],false),UvRect::from_signed([110.0000,125.0000],[11.0000,3.0000],false)] },
UvReference { source: "camel", bone: "saddle", cube: 2, nominal: [128,128], faces: [UvRect::from_signed([27.0000,89.0000],[15.0000,27.0000],false),UvRect::from_signed([42.0000,89.0000],[15.0000,27.0000],false),UvRect::from_signed([27.0000,116.0000],[15.0000,12.0000],false),UvRect::from_signed([69.0000,116.0000],[15.0000,12.0000],false),UvRect::from_signed([0.0000,116.0000],[27.0000,12.0000],false),UvRect::from_signed([42.0000,116.0000],[27.0000,12.0000],false)] },
UvReference { source: "camel", bone: "tail", cube: 0, nominal: [128,128], faces: [UvRect::from_signed([122.0000,0.0000],[3.0000,0.0000],false),UvRect::from_signed([125.0000,0.0000],[3.0000,0.0000],false),UvRect::from_signed([122.0000,0.0000],[3.0000,14.0000],false),UvRect::from_signed([125.0000,0.0000],[3.0000,14.0000],false),UvRect::from_signed([122.0000,0.0000],[0.0000,14.0000],false),UvRect::from_signed([125.0000,0.0000],[0.0000,14.0000],false)] },
UvReference { source: "camel", bone: "head", cube: 0, nominal: [128,128], faces: [UvRect::from_signed([79.0000,24.0000],[7.0000,19.0000],false),UvRect::from_signed([86.0000,24.0000],[7.0000,19.0000],false),UvRect::from_signed([79.0000,43.0000],[7.0000,8.0000],false),UvRect::from_signed([105.0000,43.0000],[7.0000,8.0000],false),UvRect::from_signed([60.0000,43.0000],[19.0000,8.0000],false),UvRect::from_signed([86.0000,43.0000],[19.0000,8.0000],false)] },
UvReference { source: "camel", bone: "head", cube: 1, nominal: [128,128], faces: [UvRect::from_signed([28.0000,0.0000],[7.0000,7.0000],false),UvRect::from_signed([35.0000,0.0000],[7.0000,7.0000],false),UvRect::from_signed([28.0000,7.0000],[7.0000,14.0000],false),UvRect::from_signed([42.0000,7.0000],[7.0000,14.0000],false),UvRect::from_signed([21.0000,7.0000],[7.0000,14.0000],false),UvRect::from_signed([35.0000,7.0000],[7.0000,14.0000],false)] },
UvReference { source: "camel", bone: "head", cube: 2, nominal: [128,128], faces: [UvRect::from_signed([56.0000,0.0000],[5.0000,6.0000],false),UvRect::from_signed([61.0000,0.0000],[5.0000,6.0000],false),UvRect::from_signed([56.0000,6.0000],[5.0000,5.0000],false),UvRect::from_signed([67.0000,6.0000],[5.0000,5.0000],false),UvRect::from_signed([50.0000,6.0000],[6.0000,5.0000],false),UvRect::from_signed([61.0000,6.0000],[6.0000,5.0000],false)] },
UvReference { source: "camel", bone: "bridle", cube: 0, nominal: [128,128], faces: [UvRect::from_signed([79.0000,87.0000],[7.0000,19.0000],false),UvRect::from_signed([86.0000,87.0000],[7.0000,19.0000],false),UvRect::from_signed([79.0000,106.0000],[7.0000,8.0000],false),UvRect::from_signed([105.0000,106.0000],[7.0000,8.0000],false),UvRect::from_signed([60.0000,106.0000],[19.0000,8.0000],false),UvRect::from_signed([86.0000,106.0000],[19.0000,8.0000],false)] },
UvReference { source: "camel", bone: "bridle", cube: 1, nominal: [128,128], faces: [UvRect::from_signed([28.0000,64.0000],[7.0000,7.0000],false),UvRect::from_signed([35.0000,64.0000],[7.0000,7.0000],false),UvRect::from_signed([28.0000,71.0000],[7.0000,14.0000],false),UvRect::from_signed([42.0000,71.0000],[7.0000,14.0000],false),UvRect::from_signed([21.0000,71.0000],[7.0000,14.0000],false),UvRect::from_signed([35.0000,71.0000],[7.0000,14.0000],false)] },
UvReference { source: "camel", bone: "bridle", cube: 2, nominal: [128,128], faces: [UvRect::from_signed([56.0000,64.0000],[5.0000,6.0000],false),UvRect::from_signed([61.0000,64.0000],[5.0000,6.0000],false),UvRect::from_signed([56.0000,70.0000],[5.0000,5.0000],false),UvRect::from_signed([67.0000,70.0000],[5.0000,5.0000],false),UvRect::from_signed([50.0000,70.0000],[6.0000,5.0000],false),UvRect::from_signed([61.0000,70.0000],[6.0000,5.0000],false)] },
UvReference { source: "camel", bone: "bridle", cube: 3, nominal: [128,128], faces: [UvRect::from_signed([76.0000,70.0000],[1.0000,2.0000],false),UvRect::from_signed([77.0000,70.0000],[1.0000,2.0000],false),UvRect::from_signed([76.0000,72.0000],[1.0000,2.0000],false),UvRect::from_signed([79.0000,72.0000],[1.0000,2.0000],false),UvRect::from_signed([74.0000,72.0000],[2.0000,2.0000],false),UvRect::from_signed([77.0000,72.0000],[2.0000,2.0000],false)] },
UvReference { source: "camel", bone: "bridle", cube: 4, nominal: [128,128], faces: [UvRect::from_signed([76.0000,70.0000],[1.0000,2.0000],true),UvRect::from_signed([77.0000,70.0000],[1.0000,2.0000],true),UvRect::from_signed([76.0000,72.0000],[1.0000,2.0000],true),UvRect::from_signed([79.0000,72.0000],[1.0000,2.0000],true),UvRect::from_signed([74.0000,72.0000],[2.0000,2.0000],true),UvRect::from_signed([77.0000,72.0000],[2.0000,2.0000],true)] },
UvReference { source: "camel", bone: "left_ear", cube: 0, nominal: [128,128], faces: [UvRect::from_signed([47.0000,0.0000],[3.0000,2.0000],false),UvRect::from_signed([50.0000,0.0000],[3.0000,2.0000],false),UvRect::from_signed([47.0000,2.0000],[3.0000,1.0000],false),UvRect::from_signed([52.0000,2.0000],[3.0000,1.0000],false),UvRect::from_signed([45.0000,2.0000],[2.0000,1.0000],false),UvRect::from_signed([50.0000,2.0000],[2.0000,1.0000],false)] },
UvReference { source: "camel", bone: "right_ear", cube: 0, nominal: [128,128], faces: [UvRect::from_signed([69.0000,0.0000],[3.0000,2.0000],false),UvRect::from_signed([72.0000,0.0000],[3.0000,2.0000],false),UvRect::from_signed([69.0000,2.0000],[3.0000,1.0000],false),UvRect::from_signed([74.0000,2.0000],[3.0000,1.0000],false),UvRect::from_signed([67.0000,2.0000],[2.0000,1.0000],false),UvRect::from_signed([72.0000,2.0000],[2.0000,1.0000],false)] },
UvReference { source: "camel", bone: "reins", cube: 0, nominal: [128,128], faces: [UvRect::from_signed([113.0000,42.0000],[0.0000,15.0000],false),UvRect::from_signed([113.0000,42.0000],[0.0000,15.0000],false),UvRect::from_signed([113.0000,57.0000],[0.0000,7.0000],false),UvRect::from_signed([128.0000,57.0000],[0.0000,7.0000],false),UvRect::from_signed([98.0000,57.0000],[15.0000,7.0000],false),UvRect::from_signed([113.0000,57.0000],[15.0000,7.0000],false)] },
UvReference { source: "camel", bone: "reins", cube: 1, nominal: [128,128], faces: [UvRect::from_signed([84.0000,57.0000],[7.4000,0.0000],false),UvRect::from_signed([91.4000,57.0000],[7.4000,0.0000],false),UvRect::from_signed([84.0000,57.0000],[7.4000,7.0000],false),UvRect::from_signed([91.4000,57.0000],[7.4000,7.0000],false),UvRect::from_signed([84.0000,57.0000],[0.0000,7.0000],false),UvRect::from_signed([91.4000,57.0000],[0.0000,7.0000],false)] },
UvReference { source: "camel", bone: "reins", cube: 2, nominal: [128,128], faces: [UvRect::from_signed([113.0000,42.0000],[0.0000,15.0000],false),UvRect::from_signed([113.0000,42.0000],[0.0000,15.0000],false),UvRect::from_signed([113.0000,57.0000],[0.0000,7.0000],false),UvRect::from_signed([128.0000,57.0000],[0.0000,7.0000],false),UvRect::from_signed([98.0000,57.0000],[15.0000,7.0000],false),UvRect::from_signed([113.0000,57.0000],[15.0000,7.0000],false)] },
UvReference { source: "camel", bone: "hump", cube: 0, nominal: [128,128], faces: [UvRect::from_signed([85.0000,0.0000],[9.0000,11.0000],false),UvRect::from_signed([94.0000,0.0000],[9.0000,11.0000],false),UvRect::from_signed([85.0000,11.0000],[9.0000,5.0000],false),UvRect::from_signed([105.0000,11.0000],[9.0000,5.0000],false),UvRect::from_signed([74.0000,11.0000],[11.0000,5.0000],false),UvRect::from_signed([94.0000,11.0000],[11.0000,5.0000],false)] },
UvReference { source: "camel", bone: "right_front_leg", cube: 0, nominal: [128,128], faces: [UvRect::from_signed([5.0000,26.0000],[5.0000,5.0000],false),UvRect::from_signed([10.0000,26.0000],[5.0000,5.0000],false),UvRect::from_signed([5.0000,31.0000],[5.0000,21.0000],false),UvRect::from_signed([15.0000,31.0000],[5.0000,21.0000],false),UvRect::from_signed([0.0000,31.0000],[5.0000,21.0000],false),UvRect::from_signed([10.0000,31.0000],[5.0000,21.0000],false)] },
UvReference { source: "camel", bone: "left_front_leg", cube: 0, nominal: [128,128], faces: [UvRect::from_signed([5.0000,0.0000],[5.0000,5.0000],false),UvRect::from_signed([10.0000,0.0000],[5.0000,5.0000],false),UvRect::from_signed([5.0000,5.0000],[5.0000,21.0000],false),UvRect::from_signed([15.0000,5.0000],[5.0000,21.0000],false),UvRect::from_signed([0.0000,5.0000],[5.0000,21.0000],false),UvRect::from_signed([10.0000,5.0000],[5.0000,21.0000],false)] },
UvReference { source: "camel", bone: "left_hind_leg", cube: 0, nominal: [128,128], faces: [UvRect::from_signed([63.0000,16.0000],[5.0000,5.0000],false),UvRect::from_signed([68.0000,16.0000],[5.0000,5.0000],false),UvRect::from_signed([63.0000,21.0000],[5.0000,21.0000],false),UvRect::from_signed([73.0000,21.0000],[5.0000,21.0000],false),UvRect::from_signed([58.0000,21.0000],[5.0000,21.0000],false),UvRect::from_signed([68.0000,21.0000],[5.0000,21.0000],false)] },
UvReference { source: "camel", bone: "right_hind_leg", cube: 0, nominal: [128,128], faces: [UvRect::from_signed([99.0000,16.0000],[5.0000,5.0000],false),UvRect::from_signed([104.0000,16.0000],[5.0000,5.0000],false),UvRect::from_signed([99.0000,21.0000],[5.0000,21.0000],false),UvRect::from_signed([109.0000,21.0000],[5.0000,21.0000],false),UvRect::from_signed([94.0000,21.0000],[5.0000,21.0000],false),UvRect::from_signed([104.0000,21.0000],[5.0000,21.0000],false)] },
UvReference { source: "cat", bone: "body", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([26.0000,0.0000],[4.0000,6.0000],false),UvRect::from_signed([30.0000,0.0000],[4.0000,6.0000],false),UvRect::from_signed([26.0000,6.0000],[4.0000,16.0000],false),UvRect::from_signed([36.0000,6.0000],[4.0000,16.0000],false),UvRect::from_signed([20.0000,6.0000],[6.0000,16.0000],false),UvRect::from_signed([30.0000,6.0000],[6.0000,16.0000],false)] },
UvReference { source: "cat", bone: "head", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([5.0000,0.0000],[5.0000,5.0000],false),UvRect::from_signed([10.0000,0.0000],[5.0000,5.0000],false),UvRect::from_signed([5.0000,5.0000],[5.0000,4.0000],false),UvRect::from_signed([15.0000,5.0000],[5.0000,4.0000],false),UvRect::from_signed([0.0000,5.0000],[5.0000,4.0000],false),UvRect::from_signed([10.0000,5.0000],[5.0000,4.0000],false)] },
UvReference { source: "cat", bone: "head", cube: 1, nominal: [64,32], faces: [UvRect::from_signed([2.0000,24.0000],[3.0000,2.0000],false),UvRect::from_signed([5.0000,24.0000],[3.0000,2.0000],false),UvRect::from_signed([2.0000,26.0000],[3.0000,2.0000],false),UvRect::from_signed([7.0000,26.0000],[3.0000,2.0000],false),UvRect::from_signed([0.0000,26.0000],[2.0000,2.0000],false),UvRect::from_signed([5.0000,26.0000],[2.0000,2.0000],false)] },
UvReference { source: "cat", bone: "head", cube: 2, nominal: [64,32], faces: [UvRect::from_signed([2.0000,10.0000],[1.0000,2.0000],false),UvRect::from_signed([3.0000,10.0000],[1.0000,2.0000],false),UvRect::from_signed([2.0000,12.0000],[1.0000,1.0000],false),UvRect::from_signed([5.0000,12.0000],[1.0000,1.0000],false),UvRect::from_signed([0.0000,12.0000],[2.0000,1.0000],false),UvRect::from_signed([3.0000,12.0000],[2.0000,1.0000],false)] },
UvReference { source: "cat", bone: "head", cube: 3, nominal: [64,32], faces: [UvRect::from_signed([8.0000,10.0000],[1.0000,2.0000],false),UvRect::from_signed([9.0000,10.0000],[1.0000,2.0000],false),UvRect::from_signed([8.0000,12.0000],[1.0000,1.0000],false),UvRect::from_signed([11.0000,12.0000],[1.0000,1.0000],false),UvRect::from_signed([6.0000,12.0000],[2.0000,1.0000],false),UvRect::from_signed([9.0000,12.0000],[2.0000,1.0000],false)] },
UvReference { source: "cat", bone: "tail1", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([1.0000,15.0000],[1.0000,1.0000],false),UvRect::from_signed([2.0000,15.0000],[1.0000,1.0000],false),UvRect::from_signed([1.0000,16.0000],[1.0000,8.0000],false),UvRect::from_signed([3.0000,16.0000],[1.0000,8.0000],false),UvRect::from_signed([0.0000,16.0000],[1.0000,8.0000],false),UvRect::from_signed([2.0000,16.0000],[1.0000,8.0000],false)] },
UvReference { source: "cat", bone: "tail2", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([5.0000,15.0000],[1.0000,1.0000],false),UvRect::from_signed([6.0000,15.0000],[1.0000,1.0000],false),UvRect::from_signed([5.0000,16.0000],[1.0000,8.0000],false),UvRect::from_signed([7.0000,16.0000],[1.0000,8.0000],false),UvRect::from_signed([4.0000,16.0000],[1.0000,8.0000],false),UvRect::from_signed([6.0000,16.0000],[1.0000,8.0000],false)] },
UvReference { source: "cat", bone: "backLegL", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([10.0000,13.0000],[2.0000,2.0000],false),UvRect::from_signed([12.0000,13.0000],[2.0000,2.0000],false),UvRect::from_signed([10.0000,15.0000],[2.0000,6.0000],false),UvRect::from_signed([14.0000,15.0000],[2.0000,6.0000],false),UvRect::from_signed([8.0000,15.0000],[2.0000,6.0000],false),UvRect::from_signed([12.0000,15.0000],[2.0000,6.0000],false)] },
UvReference { source: "cat", bone: "backLegR", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([10.0000,13.0000],[2.0000,2.0000],false),UvRect::from_signed([12.0000,13.0000],[2.0000,2.0000],false),UvRect::from_signed([10.0000,15.0000],[2.0000,6.0000],false),UvRect::from_signed([14.0000,15.0000],[2.0000,6.0000],false),UvRect::from_signed([8.0000,15.0000],[2.0000,6.0000],false),UvRect::from_signed([12.0000,15.0000],[2.0000,6.0000],false)] },
UvReference { source: "cat", bone: "frontLegL", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([42.0000,0.0000],[2.0000,2.0000],false),UvRect::from_signed([44.0000,0.0000],[2.0000,2.0000],false),UvRect::from_signed([42.0000,2.0000],[2.0000,10.0000],false),UvRect::from_signed([46.0000,2.0000],[2.0000,10.0000],false),UvRect::from_signed([40.0000,2.0000],[2.0000,10.0000],false),UvRect::from_signed([44.0000,2.0000],[2.0000,10.0000],false)] },
UvReference { source: "cat", bone: "frontLegR", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([42.0000,0.0000],[2.0000,2.0000],false),UvRect::from_signed([44.0000,0.0000],[2.0000,2.0000],false),UvRect::from_signed([42.0000,2.0000],[2.0000,10.0000],false),UvRect::from_signed([46.0000,2.0000],[2.0000,10.0000],false),UvRect::from_signed([40.0000,2.0000],[2.0000,10.0000],false),UvRect::from_signed([44.0000,2.0000],[2.0000,10.0000],false)] },
UvReference { source: "chicken", bone: "body", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([6.0000,9.0000],[6.0000,6.0000],false),UvRect::from_signed([12.0000,9.0000],[6.0000,6.0000],false),UvRect::from_signed([6.0000,15.0000],[6.0000,8.0000],false),UvRect::from_signed([18.0000,15.0000],[6.0000,8.0000],false),UvRect::from_signed([0.0000,15.0000],[6.0000,8.0000],false),UvRect::from_signed([12.0000,15.0000],[6.0000,8.0000],false)] },
UvReference { source: "chicken", bone: "head", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([3.0000,0.0000],[4.0000,3.0000],false),UvRect::from_signed([7.0000,0.0000],[4.0000,3.0000],false),UvRect::from_signed([3.0000,3.0000],[4.0000,6.0000],false),UvRect::from_signed([10.0000,3.0000],[4.0000,6.0000],false),UvRect::from_signed([0.0000,3.0000],[3.0000,6.0000],false),UvRect::from_signed([7.0000,3.0000],[3.0000,6.0000],false)] },
UvReference { source: "chicken", bone: "comb", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([16.0000,4.0000],[2.0000,2.0000],false),UvRect::from_signed([18.0000,4.0000],[2.0000,2.0000],false),UvRect::from_signed([16.0000,6.0000],[2.0000,2.0000],false),UvRect::from_signed([20.0000,6.0000],[2.0000,2.0000],false),UvRect::from_signed([14.0000,6.0000],[2.0000,2.0000],false),UvRect::from_signed([18.0000,6.0000],[2.0000,2.0000],false)] },
UvReference { source: "chicken", bone: "beak", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([16.0000,0.0000],[4.0000,2.0000],false),UvRect::from_signed([20.0000,0.0000],[4.0000,2.0000],false),UvRect::from_signed([16.0000,2.0000],[4.0000,2.0000],false),UvRect::from_signed([22.0000,2.0000],[4.0000,2.0000],false),UvRect::from_signed([14.0000,2.0000],[2.0000,2.0000],false),UvRect::from_signed([20.0000,2.0000],[2.0000,2.0000],false)] },
UvReference { source: "chicken", bone: "leg0", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([29.0000,0.0000],[3.0000,3.0000],false),UvRect::from_signed([32.0000,0.0000],[3.0000,3.0000],false),UvRect::from_signed([29.0000,3.0000],[3.0000,5.0000],false),UvRect::from_signed([35.0000,3.0000],[3.0000,5.0000],false),UvRect::from_signed([26.0000,3.0000],[3.0000,5.0000],false),UvRect::from_signed([32.0000,3.0000],[3.0000,5.0000],false)] },
UvReference { source: "chicken", bone: "leg1", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([29.0000,0.0000],[3.0000,3.0000],false),UvRect::from_signed([32.0000,0.0000],[3.0000,3.0000],false),UvRect::from_signed([29.0000,3.0000],[3.0000,5.0000],false),UvRect::from_signed([35.0000,3.0000],[3.0000,5.0000],false),UvRect::from_signed([26.0000,3.0000],[3.0000,5.0000],false),UvRect::from_signed([32.0000,3.0000],[3.0000,5.0000],false)] },
UvReference { source: "chicken", bone: "wing0", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([30.0000,13.0000],[1.0000,6.0000],false),UvRect::from_signed([31.0000,13.0000],[1.0000,6.0000],false),UvRect::from_signed([30.0000,19.0000],[1.0000,4.0000],false),UvRect::from_signed([37.0000,19.0000],[1.0000,4.0000],false),UvRect::from_signed([24.0000,19.0000],[6.0000,4.0000],false),UvRect::from_signed([31.0000,19.0000],[6.0000,4.0000],false)] },
UvReference { source: "chicken", bone: "wing1", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([30.0000,13.0000],[1.0000,6.0000],false),UvRect::from_signed([31.0000,13.0000],[1.0000,6.0000],false),UvRect::from_signed([30.0000,19.0000],[1.0000,4.0000],false),UvRect::from_signed([37.0000,19.0000],[1.0000,4.0000],false),UvRect::from_signed([24.0000,19.0000],[6.0000,4.0000],false),UvRect::from_signed([31.0000,19.0000],[6.0000,4.0000],false)] },
UvReference { source: "chicken", bone: "body", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([6.0000,9.0000],[6.0000,6.0000],false),UvRect::from_signed([12.0000,9.0000],[6.0000,6.0000],false),UvRect::from_signed([6.0000,15.0000],[6.0000,8.0000],false),UvRect::from_signed([18.0000,15.0000],[6.0000,8.0000],false),UvRect::from_signed([0.0000,15.0000],[6.0000,8.0000],false),UvRect::from_signed([12.0000,15.0000],[6.0000,8.0000],false)] },
UvReference { source: "chicken", bone: "head", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([3.0000,0.0000],[4.0000,3.0000],false),UvRect::from_signed([7.0000,0.0000],[4.0000,3.0000],false),UvRect::from_signed([3.0000,3.0000],[4.0000,6.0000],false),UvRect::from_signed([10.0000,3.0000],[4.0000,6.0000],false),UvRect::from_signed([0.0000,3.0000],[3.0000,6.0000],false),UvRect::from_signed([7.0000,3.0000],[3.0000,6.0000],false)] },
UvReference { source: "chicken", bone: "comb", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([16.0000,4.0000],[2.0000,2.0000],false),UvRect::from_signed([18.0000,4.0000],[2.0000,2.0000],false),UvRect::from_signed([16.0000,6.0000],[2.0000,2.0000],false),UvRect::from_signed([20.0000,6.0000],[2.0000,2.0000],false),UvRect::from_signed([14.0000,6.0000],[2.0000,2.0000],false),UvRect::from_signed([18.0000,6.0000],[2.0000,2.0000],false)] },
UvReference { source: "chicken", bone: "beak", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([16.0000,0.0000],[4.0000,2.0000],false),UvRect::from_signed([20.0000,0.0000],[4.0000,2.0000],false),UvRect::from_signed([16.0000,2.0000],[4.0000,2.0000],false),UvRect::from_signed([22.0000,2.0000],[4.0000,2.0000],false),UvRect::from_signed([14.0000,2.0000],[2.0000,2.0000],false),UvRect::from_signed([20.0000,2.0000],[2.0000,2.0000],false)] },
UvReference { source: "chicken", bone: "leg0", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([29.0000,0.0000],[3.0000,3.0000],false),UvRect::from_signed([32.0000,0.0000],[3.0000,3.0000],false),UvRect::from_signed([29.0000,3.0000],[3.0000,5.0000],false),UvRect::from_signed([35.0000,3.0000],[3.0000,5.0000],false),UvRect::from_signed([26.0000,3.0000],[3.0000,5.0000],false),UvRect::from_signed([32.0000,3.0000],[3.0000,5.0000],false)] },
UvReference { source: "chicken", bone: "leg1", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([29.0000,0.0000],[3.0000,3.0000],false),UvRect::from_signed([32.0000,0.0000],[3.0000,3.0000],false),UvRect::from_signed([29.0000,3.0000],[3.0000,5.0000],false),UvRect::from_signed([35.0000,3.0000],[3.0000,5.0000],false),UvRect::from_signed([26.0000,3.0000],[3.0000,5.0000],false),UvRect::from_signed([32.0000,3.0000],[3.0000,5.0000],false)] },
UvReference { source: "chicken", bone: "wing0", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([30.0000,13.0000],[1.0000,6.0000],false),UvRect::from_signed([31.0000,13.0000],[1.0000,6.0000],false),UvRect::from_signed([30.0000,19.0000],[1.0000,4.0000],false),UvRect::from_signed([37.0000,19.0000],[1.0000,4.0000],false),UvRect::from_signed([24.0000,19.0000],[6.0000,4.0000],false),UvRect::from_signed([31.0000,19.0000],[6.0000,4.0000],false)] },
UvReference { source: "chicken", bone: "wing1", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([30.0000,13.0000],[1.0000,6.0000],false),UvRect::from_signed([31.0000,13.0000],[1.0000,6.0000],false),UvRect::from_signed([30.0000,19.0000],[1.0000,4.0000],false),UvRect::from_signed([37.0000,19.0000],[1.0000,4.0000],false),UvRect::from_signed([24.0000,19.0000],[6.0000,4.0000],false),UvRect::from_signed([31.0000,19.0000],[6.0000,4.0000],false)] },
UvReference { source: "chicken.cold", bone: "body", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([6.0000,9.0000],[6.0000,6.0000],false),UvRect::from_signed([12.0000,9.0000],[6.0000,6.0000],false),UvRect::from_signed([6.0000,15.0000],[6.0000,8.0000],false),UvRect::from_signed([18.0000,15.0000],[6.0000,8.0000],false),UvRect::from_signed([0.0000,15.0000],[6.0000,8.0000],false),UvRect::from_signed([12.0000,15.0000],[6.0000,8.0000],false)] },
UvReference { source: "chicken.cold", bone: "body", cube: 1, nominal: [64,32], faces: [UvRect::from_signed([43.0000,9.0000],[0.0000,5.0000],false),UvRect::from_signed([43.0000,9.0000],[0.0000,5.0000],false),UvRect::from_signed([43.0000,14.0000],[0.0000,3.0000],false),UvRect::from_signed([48.0000,14.0000],[0.0000,3.0000],false),UvRect::from_signed([38.0000,14.0000],[5.0000,3.0000],false),UvRect::from_signed([43.0000,14.0000],[5.0000,3.0000],false)] },
UvReference { source: "chicken.cold", bone: "head", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([3.0000,0.0000],[4.0000,3.0000],false),UvRect::from_signed([7.0000,0.0000],[4.0000,3.0000],false),UvRect::from_signed([3.0000,3.0000],[4.0000,6.0000],false),UvRect::from_signed([10.0000,3.0000],[4.0000,6.0000],false),UvRect::from_signed([0.0000,3.0000],[3.0000,6.0000],false),UvRect::from_signed([7.0000,3.0000],[3.0000,6.0000],false)] },
UvReference { source: "chicken.cold", bone: "head", cube: 1, nominal: [64,32], faces: [UvRect::from_signed([48.0000,0.0000],[6.0000,4.0000],false),UvRect::from_signed([54.0000,0.0000],[6.0000,4.0000],false),UvRect::from_signed([48.0000,4.0000],[6.0000,3.0000],false),UvRect::from_signed([58.0000,4.0000],[6.0000,3.0000],false),UvRect::from_signed([44.0000,4.0000],[4.0000,3.0000],false),UvRect::from_signed([54.0000,4.0000],[4.0000,3.0000],false)] },
UvReference { source: "chicken.cold", bone: "comb", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([16.0000,4.0000],[2.0000,2.0000],false),UvRect::from_signed([18.0000,4.0000],[2.0000,2.0000],false),UvRect::from_signed([16.0000,6.0000],[2.0000,2.0000],false),UvRect::from_signed([20.0000,6.0000],[2.0000,2.0000],false),UvRect::from_signed([14.0000,6.0000],[2.0000,2.0000],false),UvRect::from_signed([18.0000,6.0000],[2.0000,2.0000],false)] },
UvReference { source: "chicken.cold", bone: "beak", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([16.0000,0.0000],[4.0000,2.0000],false),UvRect::from_signed([20.0000,0.0000],[4.0000,2.0000],false),UvRect::from_signed([16.0000,2.0000],[4.0000,2.0000],false),UvRect::from_signed([22.0000,2.0000],[4.0000,2.0000],false),UvRect::from_signed([14.0000,2.0000],[2.0000,2.0000],false),UvRect::from_signed([20.0000,2.0000],[2.0000,2.0000],false)] },
UvReference { source: "chicken.cold", bone: "leg0", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([29.0000,0.0000],[3.0000,3.0000],false),UvRect::from_signed([32.0000,0.0000],[3.0000,3.0000],false),UvRect::from_signed([29.0000,3.0000],[3.0000,5.0000],false),UvRect::from_signed([35.0000,3.0000],[3.0000,5.0000],false),UvRect::from_signed([26.0000,3.0000],[3.0000,5.0000],false),UvRect::from_signed([32.0000,3.0000],[3.0000,5.0000],false)] },
UvReference { source: "chicken.cold", bone: "leg1", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([29.0000,0.0000],[3.0000,3.0000],false),UvRect::from_signed([32.0000,0.0000],[3.0000,3.0000],false),UvRect::from_signed([29.0000,3.0000],[3.0000,5.0000],false),UvRect::from_signed([35.0000,3.0000],[3.0000,5.0000],false),UvRect::from_signed([26.0000,3.0000],[3.0000,5.0000],false),UvRect::from_signed([32.0000,3.0000],[3.0000,5.0000],false)] },
UvReference { source: "chicken.cold", bone: "wing0", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([30.0000,13.0000],[1.0000,6.0000],false),UvRect::from_signed([31.0000,13.0000],[1.0000,6.0000],false),UvRect::from_signed([30.0000,19.0000],[1.0000,4.0000],false),UvRect::from_signed([37.0000,19.0000],[1.0000,4.0000],false),UvRect::from_signed([24.0000,19.0000],[6.0000,4.0000],false),UvRect::from_signed([31.0000,19.0000],[6.0000,4.0000],false)] },
UvReference { source: "chicken.cold", bone: "wing1", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([30.0000,13.0000],[1.0000,6.0000],false),UvRect::from_signed([31.0000,13.0000],[1.0000,6.0000],false),UvRect::from_signed([30.0000,19.0000],[1.0000,4.0000],false),UvRect::from_signed([37.0000,19.0000],[1.0000,4.0000],false),UvRect::from_signed([24.0000,19.0000],[6.0000,4.0000],false),UvRect::from_signed([31.0000,19.0000],[6.0000,4.0000],false)] },
UvReference { source: "cow", bone: "body", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([28.0000,4.0000],[12.0000,10.0000],false),UvRect::from_signed([40.0000,4.0000],[12.0000,10.0000],false),UvRect::from_signed([28.0000,14.0000],[12.0000,18.0000],false),UvRect::from_signed([50.0000,14.0000],[12.0000,18.0000],false),UvRect::from_signed([18.0000,14.0000],[10.0000,18.0000],false),UvRect::from_signed([40.0000,14.0000],[10.0000,18.0000],false)] },
UvReference { source: "cow", bone: "body", cube: 1, nominal: [64,32], faces: [UvRect::from_signed([53.0000,0.0000],[4.0000,1.0000],false),UvRect::from_signed([57.0000,0.0000],[4.0000,1.0000],false),UvRect::from_signed([53.0000,1.0000],[4.0000,6.0000],false),UvRect::from_signed([58.0000,1.0000],[4.0000,6.0000],false),UvRect::from_signed([52.0000,1.0000],[1.0000,6.0000],false),UvRect::from_signed([57.0000,1.0000],[1.0000,6.0000],false)] },
UvReference { source: "cow", bone: "head", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([6.0000,0.0000],[8.0000,6.0000],false),UvRect::from_signed([14.0000,0.0000],[8.0000,6.0000],false),UvRect::from_signed([6.0000,6.0000],[8.0000,8.0000],false),UvRect::from_signed([20.0000,6.0000],[8.0000,8.0000],false),UvRect::from_signed([0.0000,6.0000],[6.0000,8.0000],false),UvRect::from_signed([14.0000,6.0000],[6.0000,8.0000],false)] },
UvReference { source: "cow", bone: "head", cube: 1, nominal: [64,32], faces: [UvRect::from_signed([23.0000,0.0000],[1.0000,1.0000],false),UvRect::from_signed([24.0000,0.0000],[1.0000,1.0000],false),UvRect::from_signed([23.0000,1.0000],[1.0000,3.0000],false),UvRect::from_signed([25.0000,1.0000],[1.0000,3.0000],false),UvRect::from_signed([22.0000,1.0000],[1.0000,3.0000],false),UvRect::from_signed([24.0000,1.0000],[1.0000,3.0000],false)] },
UvReference { source: "cow", bone: "head", cube: 2, nominal: [64,32], faces: [UvRect::from_signed([23.0000,0.0000],[1.0000,1.0000],false),UvRect::from_signed([24.0000,0.0000],[1.0000,1.0000],false),UvRect::from_signed([23.0000,1.0000],[1.0000,3.0000],false),UvRect::from_signed([25.0000,1.0000],[1.0000,3.0000],false),UvRect::from_signed([22.0000,1.0000],[1.0000,3.0000],false),UvRect::from_signed([24.0000,1.0000],[1.0000,3.0000],false)] },
UvReference { source: "cow", bone: "leg0", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([4.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([12.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([0.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([8.0000,20.0000],[4.0000,12.0000],false)] },
UvReference { source: "cow", bone: "leg1", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],true),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],true),UvRect::from_signed([4.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([12.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([0.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([8.0000,20.0000],[4.0000,12.0000],true)] },
UvReference { source: "cow", bone: "leg2", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([4.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([12.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([0.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([8.0000,20.0000],[4.0000,12.0000],false)] },
UvReference { source: "cow", bone: "leg3", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],true),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],true),UvRect::from_signed([4.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([12.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([0.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([8.0000,20.0000],[4.0000,12.0000],true)] },
UvReference { source: "cow.v2", bone: "head", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([6.0000,0.0000],[8.0000,6.0000],false),UvRect::from_signed([14.0000,0.0000],[8.0000,6.0000],false),UvRect::from_signed([6.0000,6.0000],[8.0000,8.0000],false),UvRect::from_signed([20.0000,6.0000],[8.0000,8.0000],false),UvRect::from_signed([0.0000,6.0000],[6.0000,8.0000],false),UvRect::from_signed([14.0000,6.0000],[6.0000,8.0000],false)] },
UvReference { source: "cow.v2", bone: "head", cube: 1, nominal: [64,64], faces: [UvRect::from_signed([2.0000,33.0000],[6.0000,1.0000],false),UvRect::from_signed([8.0000,33.0000],[6.0000,1.0000],false),UvRect::from_signed([2.0000,34.0000],[6.0000,3.0000],false),UvRect::from_signed([9.0000,34.0000],[6.0000,3.0000],false),UvRect::from_signed([1.0000,34.0000],[1.0000,3.0000],false),UvRect::from_signed([8.0000,34.0000],[1.0000,3.0000],false)] },
UvReference { source: "cow.v2", bone: "head", cube: 2, nominal: [64,64], faces: [UvRect::from_signed([23.0000,0.0000],[1.0000,1.0000],false),UvRect::from_signed([24.0000,0.0000],[1.0000,1.0000],false),UvRect::from_signed([23.0000,1.0000],[1.0000,3.0000],false),UvRect::from_signed([25.0000,1.0000],[1.0000,3.0000],false),UvRect::from_signed([22.0000,1.0000],[1.0000,3.0000],false),UvRect::from_signed([24.0000,1.0000],[1.0000,3.0000],false)] },
UvReference { source: "cow.v2", bone: "head", cube: 3, nominal: [64,64], faces: [UvRect::from_signed([23.0000,0.0000],[1.0000,1.0000],false),UvRect::from_signed([24.0000,0.0000],[1.0000,1.0000],false),UvRect::from_signed([23.0000,1.0000],[1.0000,3.0000],false),UvRect::from_signed([25.0000,1.0000],[1.0000,3.0000],false),UvRect::from_signed([22.0000,1.0000],[1.0000,3.0000],false),UvRect::from_signed([24.0000,1.0000],[1.0000,3.0000],false)] },
UvReference { source: "cow.v2", bone: "body", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([28.0000,4.0000],[12.0000,10.0000],false),UvRect::from_signed([40.0000,4.0000],[12.0000,10.0000],false),UvRect::from_signed([28.0000,14.0000],[12.0000,18.0000],false),UvRect::from_signed([50.0000,14.0000],[12.0000,18.0000],false),UvRect::from_signed([18.0000,14.0000],[10.0000,18.0000],false),UvRect::from_signed([40.0000,14.0000],[10.0000,18.0000],false)] },
UvReference { source: "cow.v2", bone: "body", cube: 1, nominal: [64,64], faces: [UvRect::from_signed([53.0000,0.0000],[4.0000,1.0000],false),UvRect::from_signed([57.0000,0.0000],[4.0000,1.0000],false),UvRect::from_signed([53.0000,1.0000],[4.0000,6.0000],false),UvRect::from_signed([58.0000,1.0000],[4.0000,6.0000],false),UvRect::from_signed([52.0000,1.0000],[1.0000,6.0000],false),UvRect::from_signed([57.0000,1.0000],[1.0000,6.0000],false)] },
UvReference { source: "cow.v2", bone: "leg0", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([4.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([12.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([0.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([8.0000,20.0000],[4.0000,12.0000],false)] },
UvReference { source: "cow.v2", bone: "leg1", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],true),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],true),UvRect::from_signed([4.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([12.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([0.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([8.0000,20.0000],[4.0000,12.0000],true)] },
UvReference { source: "cow.v2", bone: "leg2", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([4.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([12.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([0.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([8.0000,20.0000],[4.0000,12.0000],false)] },
UvReference { source: "cow.v2", bone: "leg3", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],true),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],true),UvRect::from_signed([4.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([12.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([0.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([8.0000,20.0000],[4.0000,12.0000],true)] },
UvReference { source: "cow.cold", bone: "head", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([6.0000,0.0000],[8.0000,6.0000],false),UvRect::from_signed([14.0000,0.0000],[8.0000,6.0000],false),UvRect::from_signed([6.0000,6.0000],[8.0000,8.0000],false),UvRect::from_signed([20.0000,6.0000],[8.0000,8.0000],false),UvRect::from_signed([0.0000,6.0000],[6.0000,8.0000],false),UvRect::from_signed([14.0000,6.0000],[6.0000,8.0000],false)] },
UvReference { source: "cow.cold", bone: "head", cube: 1, nominal: [64,64], faces: [UvRect::from_signed([2.0000,32.0000],[2.0000,2.0000],true),UvRect::from_signed([4.0000,32.0000],[2.0000,2.0000],true),UvRect::from_signed([2.0000,34.0000],[2.0000,6.0000],true),UvRect::from_signed([6.0000,34.0000],[2.0000,6.0000],true),UvRect::from_signed([0.0000,34.0000],[2.0000,6.0000],true),UvRect::from_signed([4.0000,34.0000],[2.0000,6.0000],true)] },
UvReference { source: "cow.cold", bone: "head", cube: 2, nominal: [64,64], faces: [UvRect::from_signed([10.0000,33.0000],[6.0000,1.0000],false),UvRect::from_signed([16.0000,33.0000],[6.0000,1.0000],false),UvRect::from_signed([10.0000,34.0000],[6.0000,3.0000],false),UvRect::from_signed([17.0000,34.0000],[6.0000,3.0000],false),UvRect::from_signed([9.0000,34.0000],[1.0000,3.0000],false),UvRect::from_signed([16.0000,34.0000],[1.0000,3.0000],false)] },
UvReference { source: "cow.cold", bone: "head", cube: 3, nominal: [64,64], faces: [UvRect::from_signed([2.0000,32.0000],[2.0000,2.0000],false),UvRect::from_signed([4.0000,32.0000],[2.0000,2.0000],false),UvRect::from_signed([2.0000,34.0000],[2.0000,6.0000],false),UvRect::from_signed([6.0000,34.0000],[2.0000,6.0000],false),UvRect::from_signed([0.0000,34.0000],[2.0000,6.0000],false),UvRect::from_signed([4.0000,34.0000],[2.0000,6.0000],false)] },
UvReference { source: "cow.cold", bone: "body", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([30.0000,32.0000],[12.0000,10.0000],false),UvRect::from_signed([42.0000,32.0000],[12.0000,10.0000],false),UvRect::from_signed([30.0000,42.0000],[12.0000,18.0000],false),UvRect::from_signed([52.0000,42.0000],[12.0000,18.0000],false),UvRect::from_signed([20.0000,42.0000],[10.0000,18.0000],false),UvRect::from_signed([42.0000,42.0000],[10.0000,18.0000],false)] },
UvReference { source: "cow.cold", bone: "body", cube: 1, nominal: [64,64], faces: [UvRect::from_signed([28.0000,4.0000],[12.0000,10.0000],false),UvRect::from_signed([40.0000,4.0000],[12.0000,10.0000],false),UvRect::from_signed([28.0000,14.0000],[12.0000,18.0000],false),UvRect::from_signed([50.0000,14.0000],[12.0000,18.0000],false),UvRect::from_signed([18.0000,14.0000],[10.0000,18.0000],false),UvRect::from_signed([40.0000,14.0000],[10.0000,18.0000],false)] },
UvReference { source: "cow.cold", bone: "body", cube: 2, nominal: [64,64], faces: [UvRect::from_signed([53.0000,0.0000],[4.0000,1.0000],false),UvRect::from_signed([57.0000,0.0000],[4.0000,1.0000],false),UvRect::from_signed([53.0000,1.0000],[4.0000,6.0000],false),UvRect::from_signed([58.0000,1.0000],[4.0000,6.0000],false),UvRect::from_signed([52.0000,1.0000],[1.0000,6.0000],false),UvRect::from_signed([57.0000,1.0000],[1.0000,6.0000],false)] },
UvReference { source: "cow.cold", bone: "leg0", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([4.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([12.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([0.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([8.0000,20.0000],[4.0000,12.0000],false)] },
UvReference { source: "cow.cold", bone: "leg1", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],true),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],true),UvRect::from_signed([4.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([12.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([0.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([8.0000,20.0000],[4.0000,12.0000],true)] },
UvReference { source: "cow.cold", bone: "leg2", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([4.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([12.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([0.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([8.0000,20.0000],[4.0000,12.0000],false)] },
UvReference { source: "cow.cold", bone: "leg3", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],true),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],true),UvRect::from_signed([4.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([12.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([0.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([8.0000,20.0000],[4.0000,12.0000],true)] },
UvReference { source: "cow.warm", bone: "head", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([6.0000,0.0000],[8.0000,6.0000],false),UvRect::from_signed([14.0000,0.0000],[8.0000,6.0000],false),UvRect::from_signed([6.0000,6.0000],[8.0000,8.0000],false),UvRect::from_signed([20.0000,6.0000],[8.0000,8.0000],false),UvRect::from_signed([0.0000,6.0000],[6.0000,8.0000],false),UvRect::from_signed([14.0000,6.0000],[6.0000,8.0000],false)] },
UvReference { source: "cow.warm", bone: "head", cube: 1, nominal: [64,64], faces: [UvRect::from_signed([29.0000,0.0000],[4.0000,2.0000],false),UvRect::from_signed([33.0000,0.0000],[4.0000,2.0000],false),UvRect::from_signed([29.0000,2.0000],[4.0000,2.0000],false),UvRect::from_signed([35.0000,2.0000],[4.0000,2.0000],false),UvRect::from_signed([27.0000,2.0000],[2.0000,2.0000],false),UvRect::from_signed([33.0000,2.0000],[2.0000,2.0000],false)] },
UvReference { source: "cow.warm", bone: "head", cube: 2, nominal: [64,64], faces: [UvRect::from_signed([41.0000,0.0000],[2.0000,2.0000],false),UvRect::from_signed([43.0000,0.0000],[2.0000,2.0000],false),UvRect::from_signed([41.0000,2.0000],[2.0000,2.0000],false),UvRect::from_signed([45.0000,2.0000],[2.0000,2.0000],false),UvRect::from_signed([39.0000,2.0000],[2.0000,2.0000],false),UvRect::from_signed([43.0000,2.0000],[2.0000,2.0000],false)] },
UvReference { source: "cow.warm", bone: "head", cube: 3, nominal: [64,64], faces: [UvRect::from_signed([29.0000,0.0000],[4.0000,2.0000],true),UvRect::from_signed([33.0000,0.0000],[4.0000,2.0000],true),UvRect::from_signed([29.0000,2.0000],[4.0000,2.0000],true),UvRect::from_signed([35.0000,2.0000],[4.0000,2.0000],true),UvRect::from_signed([27.0000,2.0000],[2.0000,2.0000],true),UvRect::from_signed([33.0000,2.0000],[2.0000,2.0000],true)] },
UvReference { source: "cow.warm", bone: "head", cube: 4, nominal: [64,64], faces: [UvRect::from_signed([41.0000,0.0000],[2.0000,2.0000],true),UvRect::from_signed([43.0000,0.0000],[2.0000,2.0000],true),UvRect::from_signed([41.0000,2.0000],[2.0000,2.0000],true),UvRect::from_signed([45.0000,2.0000],[2.0000,2.0000],true),UvRect::from_signed([39.0000,2.0000],[2.0000,2.0000],true),UvRect::from_signed([43.0000,2.0000],[2.0000,2.0000],true)] },
UvReference { source: "cow.warm", bone: "head", cube: 5, nominal: [64,64], faces: [UvRect::from_signed([2.0000,33.0000],[6.0000,1.0000],false),UvRect::from_signed([8.0000,33.0000],[6.0000,1.0000],false),UvRect::from_signed([2.0000,34.0000],[6.0000,3.0000],false),UvRect::from_signed([9.0000,34.0000],[6.0000,3.0000],false),UvRect::from_signed([1.0000,34.0000],[1.0000,3.0000],false),UvRect::from_signed([8.0000,34.0000],[1.0000,3.0000],false)] },
UvReference { source: "cow.warm", bone: "body", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([28.0000,4.0000],[12.0000,10.0000],false),UvRect::from_signed([40.0000,4.0000],[12.0000,10.0000],false),UvRect::from_signed([28.0000,14.0000],[12.0000,18.0000],false),UvRect::from_signed([50.0000,14.0000],[12.0000,18.0000],false),UvRect::from_signed([18.0000,14.0000],[10.0000,18.0000],false),UvRect::from_signed([40.0000,14.0000],[10.0000,18.0000],false)] },
UvReference { source: "cow.warm", bone: "body", cube: 1, nominal: [64,64], faces: [UvRect::from_signed([53.0000,0.0000],[4.0000,1.0000],false),UvRect::from_signed([57.0000,0.0000],[4.0000,1.0000],false),UvRect::from_signed([53.0000,1.0000],[4.0000,6.0000],false),UvRect::from_signed([58.0000,1.0000],[4.0000,6.0000],false),UvRect::from_signed([52.0000,1.0000],[1.0000,6.0000],false),UvRect::from_signed([57.0000,1.0000],[1.0000,6.0000],false)] },
UvReference { source: "cow.warm", bone: "leg0", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([4.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([12.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([0.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([8.0000,20.0000],[4.0000,12.0000],false)] },
UvReference { source: "cow.warm", bone: "leg1", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],true),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],true),UvRect::from_signed([4.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([12.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([0.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([8.0000,20.0000],[4.0000,12.0000],true)] },
UvReference { source: "cow.warm", bone: "leg2", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([4.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([12.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([0.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([8.0000,20.0000],[4.0000,12.0000],false)] },
UvReference { source: "cow.warm", bone: "leg3", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],true),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],true),UvRect::from_signed([4.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([12.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([0.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([8.0000,20.0000],[4.0000,12.0000],true)] },
UvReference { source: "cow_v1.0", bone: "body", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([28.0000,4.0000],[12.0000,10.0000],false),UvRect::from_signed([40.0000,4.0000],[12.0000,10.0000],false),UvRect::from_signed([28.0000,14.0000],[12.0000,18.0000],false),UvRect::from_signed([50.0000,14.0000],[12.0000,18.0000],false),UvRect::from_signed([18.0000,14.0000],[10.0000,18.0000],false),UvRect::from_signed([40.0000,14.0000],[10.0000,18.0000],false)] },
UvReference { source: "cow_v1.0", bone: "body", cube: 1, nominal: [64,32], faces: [UvRect::from_signed([53.0000,0.0000],[4.0000,1.0000],false),UvRect::from_signed([57.0000,0.0000],[4.0000,1.0000],false),UvRect::from_signed([53.0000,1.0000],[4.0000,6.0000],false),UvRect::from_signed([58.0000,1.0000],[4.0000,6.0000],false),UvRect::from_signed([52.0000,1.0000],[1.0000,6.0000],false),UvRect::from_signed([57.0000,1.0000],[1.0000,6.0000],false)] },
UvReference { source: "cow_v1.0", bone: "head", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([6.0000,0.0000],[8.0000,6.0000],false),UvRect::from_signed([14.0000,0.0000],[8.0000,6.0000],false),UvRect::from_signed([6.0000,6.0000],[8.0000,8.0000],false),UvRect::from_signed([20.0000,6.0000],[8.0000,8.0000],false),UvRect::from_signed([0.0000,6.0000],[6.0000,8.0000],false),UvRect::from_signed([14.0000,6.0000],[6.0000,8.0000],false)] },
UvReference { source: "cow_v1.0", bone: "head", cube: 1, nominal: [64,32], faces: [UvRect::from_signed([23.0000,0.0000],[1.0000,1.0000],false),UvRect::from_signed([24.0000,0.0000],[1.0000,1.0000],false),UvRect::from_signed([23.0000,1.0000],[1.0000,3.0000],false),UvRect::from_signed([25.0000,1.0000],[1.0000,3.0000],false),UvRect::from_signed([22.0000,1.0000],[1.0000,3.0000],false),UvRect::from_signed([24.0000,1.0000],[1.0000,3.0000],false)] },
UvReference { source: "cow_v1.0", bone: "head", cube: 2, nominal: [64,32], faces: [UvRect::from_signed([23.0000,0.0000],[1.0000,1.0000],false),UvRect::from_signed([24.0000,0.0000],[1.0000,1.0000],false),UvRect::from_signed([23.0000,1.0000],[1.0000,3.0000],false),UvRect::from_signed([25.0000,1.0000],[1.0000,3.0000],false),UvRect::from_signed([22.0000,1.0000],[1.0000,3.0000],false),UvRect::from_signed([24.0000,1.0000],[1.0000,3.0000],false)] },
UvReference { source: "cow_v1.0", bone: "leg0", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([4.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([12.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([0.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([8.0000,20.0000],[4.0000,12.0000],false)] },
UvReference { source: "cow_v1.0", bone: "leg1", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],true),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],true),UvRect::from_signed([4.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([12.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([0.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([8.0000,20.0000],[4.0000,12.0000],true)] },
UvReference { source: "cow_v1.0", bone: "leg2", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([4.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([12.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([0.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([8.0000,20.0000],[4.0000,12.0000],false)] },
UvReference { source: "cow_v1.0", bone: "leg3", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],true),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],true),UvRect::from_signed([4.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([12.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([0.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([8.0000,20.0000],[4.0000,12.0000],true)] },
UvReference { source: "creaking", bone: "head", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([6.0000,0.0000],[6.0000,6.0000],false),UvRect::from_signed([12.0000,0.0000],[6.0000,6.0000],false),UvRect::from_signed([6.0000,6.0000],[6.0000,10.0000],false),UvRect::from_signed([18.0000,6.0000],[6.0000,10.0000],false),UvRect::from_signed([0.0000,6.0000],[6.0000,10.0000],false),UvRect::from_signed([12.0000,6.0000],[6.0000,10.0000],false)] },
UvReference { source: "creaking", bone: "head", cube: 1, nominal: [64,64], faces: [UvRect::from_signed([34.0000,31.0000],[6.0000,6.0000],false),UvRect::from_signed([40.0000,31.0000],[6.0000,6.0000],false),UvRect::from_signed([34.0000,37.0000],[6.0000,3.0000],false),UvRect::from_signed([46.0000,37.0000],[6.0000,3.0000],false),UvRect::from_signed([28.0000,37.0000],[6.0000,3.0000],false),UvRect::from_signed([40.0000,37.0000],[6.0000,3.0000],false)] },
UvReference { source: "creaking", bone: "head", cube: 2, nominal: [64,64], faces: [UvRect::from_signed([12.0000,40.0000],[9.0000,0.0000],false),UvRect::from_signed([21.0000,40.0000],[9.0000,0.0000],false),UvRect::from_signed([12.0000,40.0000],[9.0000,14.0000],false),UvRect::from_signed([21.0000,40.0000],[9.0000,14.0000],false),UvRect::from_signed([12.0000,40.0000],[0.0000,14.0000],false),UvRect::from_signed([21.0000,40.0000],[0.0000,14.0000],false)] },
UvReference { source: "creaking", bone: "head", cube: 3, nominal: [64,64], faces: [UvRect::from_signed([34.0000,12.0000],[9.0000,0.0000],false),UvRect::from_signed([43.0000,12.0000],[9.0000,0.0000],false),UvRect::from_signed([34.0000,12.0000],[9.0000,14.0000],false),UvRect::from_signed([43.0000,12.0000],[9.0000,14.0000],false),UvRect::from_signed([34.0000,12.0000],[0.0000,14.0000],false),UvRect::from_signed([43.0000,12.0000],[0.0000,14.0000],false)] },
UvReference { source: "creaking", bone: "body", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([5.0000,16.0000],[6.0000,5.0000],false),UvRect::from_signed([11.0000,16.0000],[6.0000,5.0000],false),UvRect::from_signed([5.0000,21.0000],[6.0000,13.0000],false),UvRect::from_signed([16.0000,21.0000],[6.0000,13.0000],false),UvRect::from_signed([0.0000,21.0000],[5.0000,13.0000],false),UvRect::from_signed([11.0000,21.0000],[5.0000,13.0000],false)] },
UvReference { source: "creaking", bone: "body", cube: 1, nominal: [64,64], faces: [UvRect::from_signed([29.0000,0.0000],[6.0000,5.0000],false),UvRect::from_signed([35.0000,0.0000],[6.0000,5.0000],false),UvRect::from_signed([29.0000,5.0000],[6.0000,7.0000],false),UvRect::from_signed([40.0000,5.0000],[6.0000,7.0000],false),UvRect::from_signed([24.0000,5.0000],[5.0000,7.0000],false),UvRect::from_signed([35.0000,5.0000],[5.0000,7.0000],false)] },
UvReference { source: "creaking", bone: "rightArm", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([25.0000,13.0000],[3.0000,3.0000],false),UvRect::from_signed([28.0000,13.0000],[3.0000,3.0000],false),UvRect::from_signed([25.0000,16.0000],[3.0000,21.0000],false),UvRect::from_signed([31.0000,16.0000],[3.0000,21.0000],false),UvRect::from_signed([22.0000,16.0000],[3.0000,21.0000],false),UvRect::from_signed([28.0000,16.0000],[3.0000,21.0000],false)] },
UvReference { source: "creaking", bone: "rightArm", cube: 1, nominal: [64,64], faces: [UvRect::from_signed([49.0000,0.0000],[3.0000,3.0000],false),UvRect::from_signed([52.0000,0.0000],[3.0000,3.0000],false),UvRect::from_signed([49.0000,3.0000],[3.0000,4.0000],false),UvRect::from_signed([55.0000,3.0000],[3.0000,4.0000],false),UvRect::from_signed([46.0000,3.0000],[3.0000,4.0000],false),UvRect::from_signed([52.0000,3.0000],[3.0000,4.0000],false)] },
UvReference { source: "creaking", bone: "leftArm", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([33.0000,40.0000],[3.0000,3.0000],false),UvRect::from_signed([36.0000,40.0000],[3.0000,3.0000],false),UvRect::from_signed([33.0000,43.0000],[3.0000,16.0000],false),UvRect::from_signed([39.0000,43.0000],[3.0000,16.0000],false),UvRect::from_signed([30.0000,43.0000],[3.0000,16.0000],false),UvRect::from_signed([36.0000,43.0000],[3.0000,16.0000],false)] },
UvReference { source: "creaking", bone: "leftArm", cube: 1, nominal: [64,64], faces: [UvRect::from_signed([55.0000,12.0000],[3.0000,3.0000],false),UvRect::from_signed([58.0000,12.0000],[3.0000,3.0000],false),UvRect::from_signed([55.0000,15.0000],[3.0000,4.0000],false),UvRect::from_signed([61.0000,15.0000],[3.0000,4.0000],false),UvRect::from_signed([52.0000,15.0000],[3.0000,4.0000],false),UvRect::from_signed([58.0000,15.0000],[3.0000,4.0000],false)] },
UvReference { source: "creaking", bone: "leftArm", cube: 2, nominal: [64,64], faces: [UvRect::from_signed([55.0000,19.0000],[3.0000,3.0000],false),UvRect::from_signed([58.0000,19.0000],[3.0000,3.0000],false),UvRect::from_signed([55.0000,22.0000],[3.0000,4.0000],false),UvRect::from_signed([61.0000,22.0000],[3.0000,4.0000],false),UvRect::from_signed([52.0000,22.0000],[3.0000,4.0000],false),UvRect::from_signed([58.0000,22.0000],[3.0000,4.0000],false)] },
UvReference { source: "creaking", bone: "leftLeg", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([45.0000,40.0000],[3.0000,3.0000],false),UvRect::from_signed([48.0000,40.0000],[3.0000,3.0000],false),UvRect::from_signed([45.0000,43.0000],[3.0000,16.0000],false),UvRect::from_signed([51.0000,43.0000],[3.0000,16.0000],false),UvRect::from_signed([42.0000,43.0000],[3.0000,16.0000],false),UvRect::from_signed([48.0000,43.0000],[3.0000,16.0000],false)] },
UvReference { source: "creaking", bone: "rightLeg", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([3.0000,34.0000],[3.0000,3.0000],false),UvRect::from_signed([6.0000,34.0000],[3.0000,3.0000],false),UvRect::from_signed([3.0000,37.0000],[3.0000,19.0000],false),UvRect::from_signed([9.0000,37.0000],[3.0000,19.0000],false),UvRect::from_signed([0.0000,37.0000],[3.0000,19.0000],false),UvRect::from_signed([6.0000,37.0000],[3.0000,19.0000],false)] },
UvReference { source: "creaking", bone: "rightLeg", cube: 2, nominal: [64,64], faces: [UvRect::from_signed([15.0000,34.0000],[3.0000,3.0000],false),UvRect::from_signed([18.0000,34.0000],[3.0000,3.0000],false),UvRect::from_signed([15.0000,37.0000],[3.0000,3.0000],false),UvRect::from_signed([21.0000,37.0000],[3.0000,3.0000],false),UvRect::from_signed([12.0000,37.0000],[3.0000,3.0000],false),UvRect::from_signed([18.0000,37.0000],[3.0000,3.0000],false)] },
UvReference { source: "creeper", bone: "body", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([20.0000,16.0000],[8.0000,4.0000],false),UvRect::from_signed([28.0000,16.0000],[8.0000,4.0000],false),UvRect::from_signed([20.0000,20.0000],[8.0000,12.0000],false),UvRect::from_signed([32.0000,20.0000],[8.0000,12.0000],false),UvRect::from_signed([16.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([28.0000,20.0000],[4.0000,12.0000],false)] },
UvReference { source: "creeper", bone: "head", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([8.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([16.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([8.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([24.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([0.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([16.0000,8.0000],[8.0000,8.0000],false)] },
UvReference { source: "creeper", bone: "leg0", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([4.0000,20.0000],[4.0000,6.0000],false),UvRect::from_signed([12.0000,20.0000],[4.0000,6.0000],false),UvRect::from_signed([0.0000,20.0000],[4.0000,6.0000],false),UvRect::from_signed([8.0000,20.0000],[4.0000,6.0000],false)] },
UvReference { source: "creeper", bone: "leg1", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([4.0000,20.0000],[4.0000,6.0000],false),UvRect::from_signed([12.0000,20.0000],[4.0000,6.0000],false),UvRect::from_signed([0.0000,20.0000],[4.0000,6.0000],false),UvRect::from_signed([8.0000,20.0000],[4.0000,6.0000],false)] },
UvReference { source: "creeper", bone: "leg2", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([4.0000,20.0000],[4.0000,6.0000],false),UvRect::from_signed([12.0000,20.0000],[4.0000,6.0000],false),UvRect::from_signed([0.0000,20.0000],[4.0000,6.0000],false),UvRect::from_signed([8.0000,20.0000],[4.0000,6.0000],false)] },
UvReference { source: "creeper", bone: "leg3", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([4.0000,20.0000],[4.0000,6.0000],false),UvRect::from_signed([12.0000,20.0000],[4.0000,6.0000],false),UvRect::from_signed([0.0000,20.0000],[4.0000,6.0000],false),UvRect::from_signed([8.0000,20.0000],[4.0000,6.0000],false)] },
UvReference { source: "creeper", bone: "body", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([20.0000,16.0000],[8.0000,4.0000],false),UvRect::from_signed([28.0000,16.0000],[8.0000,4.0000],false),UvRect::from_signed([20.0000,20.0000],[8.0000,12.0000],false),UvRect::from_signed([32.0000,20.0000],[8.0000,12.0000],false),UvRect::from_signed([16.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([28.0000,20.0000],[4.0000,12.0000],false)] },
UvReference { source: "creeper", bone: "head", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([8.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([16.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([8.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([24.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([0.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([16.0000,8.0000],[8.0000,8.0000],false)] },
UvReference { source: "creeper", bone: "leg0", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([4.0000,20.0000],[4.0000,6.0000],false),UvRect::from_signed([12.0000,20.0000],[4.0000,6.0000],false),UvRect::from_signed([0.0000,20.0000],[4.0000,6.0000],false),UvRect::from_signed([8.0000,20.0000],[4.0000,6.0000],false)] },
UvReference { source: "creeper", bone: "leg1", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([4.0000,20.0000],[4.0000,6.0000],false),UvRect::from_signed([12.0000,20.0000],[4.0000,6.0000],false),UvRect::from_signed([0.0000,20.0000],[4.0000,6.0000],false),UvRect::from_signed([8.0000,20.0000],[4.0000,6.0000],false)] },
UvReference { source: "creeper", bone: "leg2", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([4.0000,20.0000],[4.0000,6.0000],false),UvRect::from_signed([12.0000,20.0000],[4.0000,6.0000],false),UvRect::from_signed([0.0000,20.0000],[4.0000,6.0000],false),UvRect::from_signed([8.0000,20.0000],[4.0000,6.0000],false)] },
UvReference { source: "creeper", bone: "leg3", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([4.0000,20.0000],[4.0000,6.0000],false),UvRect::from_signed([12.0000,20.0000],[4.0000,6.0000],false),UvRect::from_signed([0.0000,20.0000],[4.0000,6.0000],false),UvRect::from_signed([8.0000,20.0000],[4.0000,6.0000],false)] },
UvReference { source: "drowned", bone: "body", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([20.0000,16.0000],[8.0000,4.0000],false),UvRect::from_signed([28.0000,16.0000],[8.0000,4.0000],false),UvRect::from_signed([20.0000,20.0000],[8.0000,12.0000],false),UvRect::from_signed([32.0000,20.0000],[8.0000,12.0000],false),UvRect::from_signed([16.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([28.0000,20.0000],[4.0000,12.0000],false)] },
UvReference { source: "drowned", bone: "jacket", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([20.0000,32.0000],[8.0000,4.0000],false),UvRect::from_signed([28.0000,32.0000],[8.0000,4.0000],false),UvRect::from_signed([20.0000,36.0000],[8.0000,12.0000],false),UvRect::from_signed([32.0000,36.0000],[8.0000,12.0000],false),UvRect::from_signed([16.0000,36.0000],[4.0000,12.0000],false),UvRect::from_signed([28.0000,36.0000],[4.0000,12.0000],false)] },
UvReference { source: "drowned", bone: "head", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([8.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([16.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([8.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([24.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([0.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([16.0000,8.0000],[8.0000,8.0000],false)] },
UvReference { source: "drowned", bone: "hat", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([40.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([48.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([40.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([56.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([32.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([48.0000,8.0000],[8.0000,8.0000],false)] },
UvReference { source: "drowned", bone: "rightArm", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([4.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([12.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([0.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([8.0000,20.0000],[4.0000,12.0000],false)] },
UvReference { source: "drowned", bone: "rightSleeve", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([52.0000,48.0000],[4.0000,4.0000],false),UvRect::from_signed([56.0000,48.0000],[4.0000,4.0000],false),UvRect::from_signed([52.0000,52.0000],[4.0000,12.0000],false),UvRect::from_signed([60.0000,52.0000],[4.0000,12.0000],false),UvRect::from_signed([48.0000,52.0000],[4.0000,12.0000],false),UvRect::from_signed([56.0000,52.0000],[4.0000,12.0000],false)] },
UvReference { source: "drowned", bone: "leftArm", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([44.0000,16.0000],[4.0000,4.0000],true),UvRect::from_signed([48.0000,16.0000],[4.0000,4.0000],true),UvRect::from_signed([44.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([52.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([40.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([48.0000,20.0000],[4.0000,12.0000],true)] },
UvReference { source: "drowned", bone: "leftSleeve", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([44.0000,32.0000],[4.0000,4.0000],true),UvRect::from_signed([48.0000,32.0000],[4.0000,4.0000],true),UvRect::from_signed([44.0000,36.0000],[4.0000,12.0000],true),UvRect::from_signed([52.0000,36.0000],[4.0000,12.0000],true),UvRect::from_signed([40.0000,36.0000],[4.0000,12.0000],true),UvRect::from_signed([48.0000,36.0000],[4.0000,12.0000],true)] },
UvReference { source: "drowned", bone: "rightLeg", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([20.0000,48.0000],[4.0000,4.0000],false),UvRect::from_signed([24.0000,48.0000],[4.0000,4.0000],false),UvRect::from_signed([20.0000,52.0000],[4.0000,12.0000],false),UvRect::from_signed([28.0000,52.0000],[4.0000,12.0000],false),UvRect::from_signed([16.0000,52.0000],[4.0000,12.0000],false),UvRect::from_signed([24.0000,52.0000],[4.0000,12.0000],false)] },
UvReference { source: "drowned", bone: "rightPants", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([4.0000,48.0000],[4.0000,4.0000],false),UvRect::from_signed([8.0000,48.0000],[4.0000,4.0000],false),UvRect::from_signed([4.0000,52.0000],[4.0000,12.0000],false),UvRect::from_signed([12.0000,52.0000],[4.0000,12.0000],false),UvRect::from_signed([0.0000,52.0000],[4.0000,12.0000],false),UvRect::from_signed([8.0000,52.0000],[4.0000,12.0000],false)] },
UvReference { source: "drowned", bone: "leftLeg", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([36.0000,48.0000],[4.0000,4.0000],true),UvRect::from_signed([40.0000,48.0000],[4.0000,4.0000],true),UvRect::from_signed([36.0000,52.0000],[4.0000,12.0000],true),UvRect::from_signed([44.0000,52.0000],[4.0000,12.0000],true),UvRect::from_signed([32.0000,52.0000],[4.0000,12.0000],true),UvRect::from_signed([40.0000,52.0000],[4.0000,12.0000],true)] },
UvReference { source: "drowned", bone: "leftPants", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([4.0000,32.0000],[4.0000,4.0000],true),UvRect::from_signed([8.0000,32.0000],[4.0000,4.0000],true),UvRect::from_signed([4.0000,36.0000],[4.0000,12.0000],true),UvRect::from_signed([12.0000,36.0000],[4.0000,12.0000],true),UvRect::from_signed([0.0000,36.0000],[4.0000,12.0000],true),UvRect::from_signed([8.0000,36.0000],[4.0000,12.0000],true)] },
UvReference { source: "enderman", bone: "hat", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([8.0000,16.0000],[8.0000,8.0000],false),UvRect::from_signed([16.0000,16.0000],[8.0000,8.0000],false),UvRect::from_signed([8.0000,24.0000],[8.0000,8.0000],false),UvRect::from_signed([24.0000,24.0000],[8.0000,8.0000],false),UvRect::from_signed([0.0000,24.0000],[8.0000,8.0000],false),UvRect::from_signed([16.0000,24.0000],[8.0000,8.0000],false)] },
UvReference { source: "enderman", bone: "head", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([8.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([16.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([8.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([24.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([0.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([16.0000,8.0000],[8.0000,8.0000],false)] },
UvReference { source: "enderman", bone: "body", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([36.0000,16.0000],[8.0000,4.0000],false),UvRect::from_signed([44.0000,16.0000],[8.0000,4.0000],false),UvRect::from_signed([36.0000,20.0000],[8.0000,12.0000],false),UvRect::from_signed([48.0000,20.0000],[8.0000,12.0000],false),UvRect::from_signed([32.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([44.0000,20.0000],[4.0000,12.0000],false)] },
UvReference { source: "enderman", bone: "rightArm", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([58.0000,0.0000],[2.0000,2.0000],false),UvRect::from_signed([60.0000,0.0000],[2.0000,2.0000],false),UvRect::from_signed([58.0000,2.0000],[2.0000,30.0000],false),UvRect::from_signed([62.0000,2.0000],[2.0000,30.0000],false),UvRect::from_signed([56.0000,2.0000],[2.0000,30.0000],false),UvRect::from_signed([60.0000,2.0000],[2.0000,30.0000],false)] },
UvReference { source: "enderman", bone: "leftArm", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([58.0000,0.0000],[2.0000,2.0000],true),UvRect::from_signed([60.0000,0.0000],[2.0000,2.0000],true),UvRect::from_signed([58.0000,2.0000],[2.0000,30.0000],true),UvRect::from_signed([62.0000,2.0000],[2.0000,30.0000],true),UvRect::from_signed([56.0000,2.0000],[2.0000,30.0000],true),UvRect::from_signed([60.0000,2.0000],[2.0000,30.0000],true)] },
UvReference { source: "enderman", bone: "rightLeg", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([58.0000,0.0000],[2.0000,2.0000],false),UvRect::from_signed([60.0000,0.0000],[2.0000,2.0000],false),UvRect::from_signed([58.0000,2.0000],[2.0000,30.0000],false),UvRect::from_signed([62.0000,2.0000],[2.0000,30.0000],false),UvRect::from_signed([56.0000,2.0000],[2.0000,30.0000],false),UvRect::from_signed([60.0000,2.0000],[2.0000,30.0000],false)] },
UvReference { source: "enderman", bone: "leftLeg", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([58.0000,0.0000],[2.0000,2.0000],true),UvRect::from_signed([60.0000,0.0000],[2.0000,2.0000],true),UvRect::from_signed([58.0000,2.0000],[2.0000,30.0000],true),UvRect::from_signed([62.0000,2.0000],[2.0000,30.0000],true),UvRect::from_signed([56.0000,2.0000],[2.0000,30.0000],true),UvRect::from_signed([60.0000,2.0000],[2.0000,30.0000],true)] },
UvReference { source: "evoker", bone: "head", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([8.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([16.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([8.0000,8.0000],[8.0000,10.0000],false),UvRect::from_signed([24.0000,8.0000],[8.0000,10.0000],false),UvRect::from_signed([0.0000,8.0000],[8.0000,10.0000],false),UvRect::from_signed([16.0000,8.0000],[8.0000,10.0000],false)] },
UvReference { source: "evoker", bone: "nose", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([26.0000,0.0000],[2.0000,2.0000],false),UvRect::from_signed([28.0000,0.0000],[2.0000,2.0000],false),UvRect::from_signed([26.0000,2.0000],[2.0000,4.0000],false),UvRect::from_signed([30.0000,2.0000],[2.0000,4.0000],false),UvRect::from_signed([24.0000,2.0000],[2.0000,4.0000],false),UvRect::from_signed([28.0000,2.0000],[2.0000,4.0000],false)] },
UvReference { source: "evoker", bone: "body", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([22.0000,20.0000],[8.0000,6.0000],false),UvRect::from_signed([30.0000,20.0000],[8.0000,6.0000],false),UvRect::from_signed([22.0000,26.0000],[8.0000,12.0000],false),UvRect::from_signed([36.0000,26.0000],[8.0000,12.0000],false),UvRect::from_signed([16.0000,26.0000],[6.0000,12.0000],false),UvRect::from_signed([30.0000,26.0000],[6.0000,12.0000],false)] },
UvReference { source: "evoker", bone: "body", cube: 1, nominal: [64,64], faces: [UvRect::from_signed([6.0000,38.0000],[8.0000,6.0000],false),UvRect::from_signed([14.0000,38.0000],[8.0000,6.0000],false),UvRect::from_signed([6.0000,44.0000],[8.0000,18.0000],false),UvRect::from_signed([20.0000,44.0000],[8.0000,18.0000],false),UvRect::from_signed([0.0000,44.0000],[6.0000,18.0000],false),UvRect::from_signed([14.0000,44.0000],[6.0000,18.0000],false)] },
UvReference { source: "evoker", bone: "leftLeg", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([4.0000,22.0000],[4.0000,4.0000],false),UvRect::from_signed([8.0000,22.0000],[4.0000,4.0000],false),UvRect::from_signed([4.0000,26.0000],[4.0000,12.0000],false),UvRect::from_signed([12.0000,26.0000],[4.0000,12.0000],false),UvRect::from_signed([0.0000,26.0000],[4.0000,12.0000],false),UvRect::from_signed([8.0000,26.0000],[4.0000,12.0000],false)] },
UvReference { source: "evoker", bone: "rightLeg", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([4.0000,22.0000],[4.0000,4.0000],true),UvRect::from_signed([8.0000,22.0000],[4.0000,4.0000],true),UvRect::from_signed([4.0000,26.0000],[4.0000,12.0000],true),UvRect::from_signed([12.0000,26.0000],[4.0000,12.0000],true),UvRect::from_signed([0.0000,26.0000],[4.0000,12.0000],true),UvRect::from_signed([8.0000,26.0000],[4.0000,12.0000],true)] },
UvReference { source: "evoker", bone: "arms", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([48.0000,22.0000],[4.0000,4.0000],false),UvRect::from_signed([52.0000,22.0000],[4.0000,4.0000],false),UvRect::from_signed([48.0000,26.0000],[4.0000,8.0000],false),UvRect::from_signed([56.0000,26.0000],[4.0000,8.0000],false),UvRect::from_signed([44.0000,26.0000],[4.0000,8.0000],false),UvRect::from_signed([52.0000,26.0000],[4.0000,8.0000],false)] },
UvReference { source: "evoker", bone: "arms", cube: 1, nominal: [64,64], faces: [UvRect::from_signed([48.0000,22.0000],[4.0000,4.0000],false),UvRect::from_signed([52.0000,22.0000],[4.0000,4.0000],false),UvRect::from_signed([48.0000,26.0000],[4.0000,8.0000],false),UvRect::from_signed([56.0000,26.0000],[4.0000,8.0000],false),UvRect::from_signed([44.0000,26.0000],[4.0000,8.0000],false),UvRect::from_signed([52.0000,26.0000],[4.0000,8.0000],false)] },
UvReference { source: "evoker", bone: "arms", cube: 2, nominal: [64,64], faces: [UvRect::from_signed([44.0000,38.0000],[8.0000,4.0000],false),UvRect::from_signed([52.0000,38.0000],[8.0000,4.0000],false),UvRect::from_signed([44.0000,42.0000],[8.0000,4.0000],false),UvRect::from_signed([56.0000,42.0000],[8.0000,4.0000],false),UvRect::from_signed([40.0000,42.0000],[4.0000,4.0000],false),UvRect::from_signed([52.0000,42.0000],[4.0000,4.0000],false)] },
UvReference { source: "evoker", bone: "rightArm", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([44.0000,46.0000],[4.0000,4.0000],false),UvRect::from_signed([48.0000,46.0000],[4.0000,4.0000],false),UvRect::from_signed([44.0000,50.0000],[4.0000,12.0000],false),UvRect::from_signed([52.0000,50.0000],[4.0000,12.0000],false),UvRect::from_signed([40.0000,50.0000],[4.0000,12.0000],false),UvRect::from_signed([48.0000,50.0000],[4.0000,12.0000],false)] },
UvReference { source: "evoker", bone: "leftArm", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([44.0000,46.0000],[4.0000,4.0000],true),UvRect::from_signed([48.0000,46.0000],[4.0000,4.0000],true),UvRect::from_signed([44.0000,50.0000],[4.0000,12.0000],true),UvRect::from_signed([52.0000,50.0000],[4.0000,12.0000],true),UvRect::from_signed([40.0000,50.0000],[4.0000,12.0000],true),UvRect::from_signed([48.0000,50.0000],[4.0000,12.0000],true)] },
UvReference { source: "fox", bone: "body", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([36.0000,15.0000],[6.0000,6.0000],false),UvRect::from_signed([42.0000,15.0000],[6.0000,6.0000],false),UvRect::from_signed([36.0000,21.0000],[6.0000,11.0000],false),UvRect::from_signed([48.0000,21.0000],[6.0000,11.0000],false),UvRect::from_signed([30.0000,21.0000],[6.0000,11.0000],false),UvRect::from_signed([42.0000,21.0000],[6.0000,11.0000],false)] },
UvReference { source: "fox", bone: "head", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([6.0000,0.0000],[8.0000,6.0000],false),UvRect::from_signed([14.0000,0.0000],[8.0000,6.0000],false),UvRect::from_signed([6.0000,6.0000],[8.0000,6.0000],false),UvRect::from_signed([20.0000,6.0000],[8.0000,6.0000],false),UvRect::from_signed([0.0000,6.0000],[6.0000,6.0000],false),UvRect::from_signed([14.0000,6.0000],[6.0000,6.0000],false)] },
UvReference { source: "fox", bone: "head", cube: 1, nominal: [64,32], faces: [UvRect::from_signed([1.0000,0.0000],[2.0000,1.0000],false),UvRect::from_signed([3.0000,0.0000],[2.0000,1.0000],false),UvRect::from_signed([1.0000,1.0000],[2.0000,2.0000],false),UvRect::from_signed([4.0000,1.0000],[2.0000,2.0000],false),UvRect::from_signed([0.0000,1.0000],[1.0000,2.0000],false),UvRect::from_signed([3.0000,1.0000],[1.0000,2.0000],false)] },
UvReference { source: "fox", bone: "head", cube: 2, nominal: [64,32], faces: [UvRect::from_signed([23.0000,0.0000],[2.0000,1.0000],false),UvRect::from_signed([25.0000,0.0000],[2.0000,1.0000],false),UvRect::from_signed([23.0000,1.0000],[2.0000,2.0000],false),UvRect::from_signed([26.0000,1.0000],[2.0000,2.0000],false),UvRect::from_signed([22.0000,1.0000],[1.0000,2.0000],false),UvRect::from_signed([25.0000,1.0000],[1.0000,2.0000],false)] },
UvReference { source: "fox", bone: "head", cube: 3, nominal: [64,32], faces: [UvRect::from_signed([3.0000,24.0000],[4.0000,3.0000],false),UvRect::from_signed([7.0000,24.0000],[4.0000,3.0000],false),UvRect::from_signed([3.0000,27.0000],[4.0000,2.0000],false),UvRect::from_signed([10.0000,27.0000],[4.0000,2.0000],false),UvRect::from_signed([0.0000,27.0000],[3.0000,2.0000],false),UvRect::from_signed([7.0000,27.0000],[3.0000,2.0000],false)] },
UvReference { source: "fox", bone: "head_sleeping", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([6.0000,12.0000],[8.0000,6.0000],false),UvRect::from_signed([14.0000,12.0000],[8.0000,6.0000],false),UvRect::from_signed([6.0000,18.0000],[8.0000,6.0000],false),UvRect::from_signed([20.0000,18.0000],[8.0000,6.0000],false),UvRect::from_signed([0.0000,18.0000],[6.0000,6.0000],false),UvRect::from_signed([14.0000,18.0000],[6.0000,6.0000],false)] },
UvReference { source: "fox", bone: "head_sleeping", cube: 1, nominal: [64,32], faces: [UvRect::from_signed([1.0000,0.0000],[2.0000,1.0000],false),UvRect::from_signed([3.0000,0.0000],[2.0000,1.0000],false),UvRect::from_signed([1.0000,1.0000],[2.0000,2.0000],false),UvRect::from_signed([4.0000,1.0000],[2.0000,2.0000],false),UvRect::from_signed([0.0000,1.0000],[1.0000,2.0000],false),UvRect::from_signed([3.0000,1.0000],[1.0000,2.0000],false)] },
UvReference { source: "fox", bone: "head_sleeping", cube: 2, nominal: [64,32], faces: [UvRect::from_signed([23.0000,0.0000],[2.0000,1.0000],false),UvRect::from_signed([25.0000,0.0000],[2.0000,1.0000],false),UvRect::from_signed([23.0000,1.0000],[2.0000,2.0000],false),UvRect::from_signed([26.0000,1.0000],[2.0000,2.0000],false),UvRect::from_signed([22.0000,1.0000],[1.0000,2.0000],false),UvRect::from_signed([25.0000,1.0000],[1.0000,2.0000],false)] },
UvReference { source: "fox", bone: "head_sleeping", cube: 3, nominal: [64,32], faces: [UvRect::from_signed([3.0000,24.0000],[4.0000,3.0000],false),UvRect::from_signed([7.0000,24.0000],[4.0000,3.0000],false),UvRect::from_signed([3.0000,27.0000],[4.0000,2.0000],false),UvRect::from_signed([10.0000,27.0000],[4.0000,2.0000],false),UvRect::from_signed([0.0000,27.0000],[3.0000,2.0000],false),UvRect::from_signed([7.0000,27.0000],[3.0000,2.0000],false)] },
UvReference { source: "fox", bone: "leg0", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([16.0000,24.0000],[2.0000,2.0000],false),UvRect::from_signed([18.0000,24.0000],[2.0000,2.0000],false),UvRect::from_signed([16.0000,26.0000],[2.0000,6.0000],false),UvRect::from_signed([20.0000,26.0000],[2.0000,6.0000],false),UvRect::from_signed([14.0000,26.0000],[2.0000,6.0000],false),UvRect::from_signed([18.0000,26.0000],[2.0000,6.0000],false)] },
UvReference { source: "fox", bone: "leg1", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([24.0000,24.0000],[2.0000,2.0000],false),UvRect::from_signed([26.0000,24.0000],[2.0000,2.0000],false),UvRect::from_signed([24.0000,26.0000],[2.0000,6.0000],false),UvRect::from_signed([28.0000,26.0000],[2.0000,6.0000],false),UvRect::from_signed([22.0000,26.0000],[2.0000,6.0000],false),UvRect::from_signed([26.0000,26.0000],[2.0000,6.0000],false)] },
UvReference { source: "fox", bone: "leg2", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([16.0000,24.0000],[2.0000,2.0000],false),UvRect::from_signed([18.0000,24.0000],[2.0000,2.0000],false),UvRect::from_signed([16.0000,26.0000],[2.0000,6.0000],false),UvRect::from_signed([20.0000,26.0000],[2.0000,6.0000],false),UvRect::from_signed([14.0000,26.0000],[2.0000,6.0000],false),UvRect::from_signed([18.0000,26.0000],[2.0000,6.0000],false)] },
UvReference { source: "fox", bone: "leg3", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([24.0000,24.0000],[2.0000,2.0000],false),UvRect::from_signed([26.0000,24.0000],[2.0000,2.0000],false),UvRect::from_signed([24.0000,26.0000],[2.0000,6.0000],false),UvRect::from_signed([28.0000,26.0000],[2.0000,6.0000],false),UvRect::from_signed([22.0000,26.0000],[2.0000,6.0000],false),UvRect::from_signed([26.0000,26.0000],[2.0000,6.0000],false)] },
UvReference { source: "fox", bone: "tail", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([33.0000,0.0000],[4.0000,5.0000],false),UvRect::from_signed([37.0000,0.0000],[4.0000,5.0000],false),UvRect::from_signed([33.0000,5.0000],[4.0000,9.0000],false),UvRect::from_signed([42.0000,5.0000],[4.0000,9.0000],false),UvRect::from_signed([28.0000,5.0000],[5.0000,9.0000],false),UvRect::from_signed([37.0000,5.0000],[5.0000,9.0000],false)] },
UvReference { source: "frog", bone: "body", cube: 0, nominal: [48,48], faces: [UvRect::from_signed([12.0000,1.0000],[7.0000,9.0000],false),UvRect::from_signed([19.0000,1.0000],[7.0000,9.0000],false),UvRect::from_signed([12.0000,10.0000],[7.0000,3.0000],false),UvRect::from_signed([28.0000,10.0000],[7.0000,3.0000],false),UvRect::from_signed([3.0000,10.0000],[9.0000,3.0000],false),UvRect::from_signed([19.0000,10.0000],[9.0000,3.0000],false)] },
UvReference { source: "frog", bone: "head", cube: 1, nominal: [48,48], faces: [UvRect::from_signed([9.0000,13.0000],[7.0000,9.0000],false),UvRect::from_signed([16.0000,13.0000],[7.0000,9.0000],false),UvRect::from_signed([9.0000,22.0000],[7.0000,3.0000],false),UvRect::from_signed([25.0000,22.0000],[7.0000,3.0000],false),UvRect::from_signed([0.0000,22.0000],[9.0000,3.0000],false),UvRect::from_signed([16.0000,22.0000],[9.0000,3.0000],false)] },
UvReference { source: "frog", bone: "right_eye", cube: 0, nominal: [48,48], faces: [UvRect::from_signed([3.0000,0.0000],[3.0000,3.0000],false),UvRect::from_signed([6.0000,0.0000],[3.0000,3.0000],false),UvRect::from_signed([3.0000,3.0000],[3.0000,2.0000],false),UvRect::from_signed([9.0000,3.0000],[3.0000,2.0000],false),UvRect::from_signed([0.0000,3.0000],[3.0000,2.0000],false),UvRect::from_signed([6.0000,3.0000],[3.0000,2.0000],false)] },
UvReference { source: "frog", bone: "left_eye", cube: 0, nominal: [48,48], faces: [UvRect::from_signed([3.0000,5.0000],[3.0000,3.0000],false),UvRect::from_signed([6.0000,5.0000],[3.0000,3.0000],false),UvRect::from_signed([3.0000,8.0000],[3.0000,2.0000],false),UvRect::from_signed([9.0000,8.0000],[3.0000,2.0000],false),UvRect::from_signed([0.0000,8.0000],[3.0000,2.0000],false),UvRect::from_signed([6.0000,8.0000],[3.0000,2.0000],false)] },
UvReference { source: "frog", bone: "croaking_body", cube: 0, nominal: [48,48], faces: [UvRect::from_signed([29.0000,5.0000],[7.0000,3.0000],false),UvRect::from_signed([36.0000,5.0000],[7.0000,3.0000],false),UvRect::from_signed([29.0000,8.0000],[7.0000,2.0000],false),UvRect::from_signed([39.0000,8.0000],[7.0000,2.0000],false),UvRect::from_signed([26.0000,8.0000],[3.0000,2.0000],false),UvRect::from_signed([36.0000,8.0000],[3.0000,2.0000],false)] },
UvReference { source: "frog", bone: "tongue", cube: 0, nominal: [48,48], faces: [UvRect::from_signed([24.0000,13.0000],[4.0000,7.0000],false),UvRect::from_signed([28.0000,13.0000],[4.0000,7.0000],false),UvRect::from_signed([24.0000,20.0000],[4.0000,0.0000],false),UvRect::from_signed([35.0000,20.0000],[4.0000,0.0000],false),UvRect::from_signed([17.0000,20.0000],[7.0000,0.0000],false),UvRect::from_signed([28.0000,20.0000],[7.0000,0.0000],false)] },
UvReference { source: "frog", bone: "left_arm", cube: 0, nominal: [48,48], faces: [UvRect::from_signed([3.0000,32.0000],[2.0000,3.0000],false),UvRect::from_signed([5.0000,32.0000],[2.0000,3.0000],false),UvRect::from_signed([3.0000,35.0000],[2.0000,3.0000],false),UvRect::from_signed([8.0000,35.0000],[2.0000,3.0000],false),UvRect::from_signed([0.0000,35.0000],[3.0000,3.0000],false),UvRect::from_signed([5.0000,35.0000],[3.0000,3.0000],false)] },
UvReference { source: "frog", bone: "right_arm", cube: 0, nominal: [48,48], faces: [UvRect::from_signed([3.0000,38.0000],[2.0000,3.0000],false),UvRect::from_signed([5.0000,38.0000],[2.0000,3.0000],false),UvRect::from_signed([3.0000,41.0000],[2.0000,3.0000],false),UvRect::from_signed([8.0000,41.0000],[2.0000,3.0000],false),UvRect::from_signed([0.0000,41.0000],[3.0000,3.0000],false),UvRect::from_signed([5.0000,41.0000],[3.0000,3.0000],false)] },
UvReference { source: "frog", bone: "right_arm", cube: 1, nominal: [48,48], faces: [UvRect::from_signed([10.0000,40.0000],[8.0000,8.0000],false),UvRect::from_signed([18.0000,40.0000],[8.0000,8.0000],false),UvRect::from_signed([10.0000,48.0000],[8.0000,0.0000],false),UvRect::from_signed([26.0000,48.0000],[8.0000,0.0000],false),UvRect::from_signed([2.0000,48.0000],[8.0000,0.0000],false),UvRect::from_signed([18.0000,48.0000],[8.0000,0.0000],false)] },
UvReference { source: "frog", bone: "left_leg", cube: 0, nominal: [48,48], faces: [UvRect::from_signed([18.0000,25.0000],[3.0000,4.0000],false),UvRect::from_signed([21.0000,25.0000],[3.0000,4.0000],false),UvRect::from_signed([18.0000,29.0000],[3.0000,3.0000],false),UvRect::from_signed([25.0000,29.0000],[3.0000,3.0000],false),UvRect::from_signed([14.0000,29.0000],[4.0000,3.0000],false),UvRect::from_signed([21.0000,29.0000],[4.0000,3.0000],false)] },
UvReference { source: "frog", bone: "left_leg", cube: 1, nominal: [48,48], faces: [UvRect::from_signed([10.0000,32.0000],[8.0000,8.0000],false),UvRect::from_signed([18.0000,32.0000],[8.0000,8.0000],false),UvRect::from_signed([10.0000,40.0000],[8.0000,0.0000],false),UvRect::from_signed([26.0000,40.0000],[8.0000,0.0000],false),UvRect::from_signed([2.0000,40.0000],[8.0000,0.0000],false),UvRect::from_signed([18.0000,40.0000],[8.0000,0.0000],false)] },
UvReference { source: "frog", bone: "right_leg", cube: 0, nominal: [48,48], faces: [UvRect::from_signed([4.0000,25.0000],[3.0000,4.0000],false),UvRect::from_signed([7.0000,25.0000],[3.0000,4.0000],false),UvRect::from_signed([4.0000,29.0000],[3.0000,3.0000],false),UvRect::from_signed([11.0000,29.0000],[3.0000,3.0000],false),UvRect::from_signed([0.0000,29.0000],[4.0000,3.0000],false),UvRect::from_signed([7.0000,29.0000],[4.0000,3.0000],false)] },
UvReference { source: "goat", bone: "left_back_leg", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([39.0000,29.0000],[3.0000,3.0000],false),UvRect::from_signed([42.0000,29.0000],[3.0000,3.0000],false),UvRect::from_signed([39.0000,32.0000],[3.0000,6.0000],false),UvRect::from_signed([45.0000,32.0000],[3.0000,6.0000],false),UvRect::from_signed([36.0000,32.0000],[3.0000,6.0000],false),UvRect::from_signed([42.0000,32.0000],[3.0000,6.0000],false)] },
UvReference { source: "goat", bone: "right_back_leg", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([52.0000,29.0000],[3.0000,3.0000],false),UvRect::from_signed([55.0000,29.0000],[3.0000,3.0000],false),UvRect::from_signed([52.0000,32.0000],[3.0000,6.0000],false),UvRect::from_signed([58.0000,32.0000],[3.0000,6.0000],false),UvRect::from_signed([49.0000,32.0000],[3.0000,6.0000],false),UvRect::from_signed([55.0000,32.0000],[3.0000,6.0000],false)] },
UvReference { source: "goat", bone: "right_front_leg", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([52.0000,2.0000],[3.0000,3.0000],false),UvRect::from_signed([55.0000,2.0000],[3.0000,3.0000],false),UvRect::from_signed([52.0000,5.0000],[3.0000,10.0000],false),UvRect::from_signed([58.0000,5.0000],[3.0000,10.0000],false),UvRect::from_signed([49.0000,5.0000],[3.0000,10.0000],false),UvRect::from_signed([55.0000,5.0000],[3.0000,10.0000],false)] },
UvReference { source: "goat", bone: "left_front_leg", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([38.0000,2.0000],[3.0000,3.0000],false),UvRect::from_signed([41.0000,2.0000],[3.0000,3.0000],false),UvRect::from_signed([38.0000,5.0000],[3.0000,10.0000],false),UvRect::from_signed([44.0000,5.0000],[3.0000,10.0000],false),UvRect::from_signed([35.0000,5.0000],[3.0000,10.0000],false),UvRect::from_signed([41.0000,5.0000],[3.0000,10.0000],false)] },
UvReference { source: "goat", bone: "body", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([17.0000,1.0000],[9.0000,16.0000],false),UvRect::from_signed([26.0000,1.0000],[9.0000,16.0000],false),UvRect::from_signed([17.0000,17.0000],[9.0000,11.0000],false),UvRect::from_signed([42.0000,17.0000],[9.0000,11.0000],false),UvRect::from_signed([1.0000,17.0000],[16.0000,11.0000],false),UvRect::from_signed([26.0000,17.0000],[16.0000,11.0000],false)] },
UvReference { source: "goat", bone: "body", cube: 1, nominal: [64,64], faces: [UvRect::from_signed([11.0000,28.0000],[11.0000,11.0000],false),UvRect::from_signed([22.0000,28.0000],[11.0000,11.0000],false),UvRect::from_signed([11.0000,39.0000],[11.0000,14.0000],false),UvRect::from_signed([33.0000,39.0000],[11.0000,14.0000],false),UvRect::from_signed([0.0000,39.0000],[11.0000,14.0000],false),UvRect::from_signed([22.0000,39.0000],[11.0000,14.0000],false)] },
UvReference { source: "goat", bone: "head", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([44.0000,46.0000],[5.0000,10.0000],false),UvRect::from_signed([49.0000,46.0000],[5.0000,10.0000],false),UvRect::from_signed([44.0000,56.0000],[5.0000,7.0000],false),UvRect::from_signed([59.0000,56.0000],[5.0000,7.0000],false),UvRect::from_signed([34.0000,56.0000],[10.0000,7.0000],false),UvRect::from_signed([49.0000,56.0000],[10.0000,7.0000],false)] },
UvReference { source: "goat", bone: "head", cube: 1, nominal: [64,64], faces: [UvRect::from_signed([3.0000,61.0000],[3.0000,1.0000],true),UvRect::from_signed([6.0000,61.0000],[3.0000,1.0000],true),UvRect::from_signed([3.0000,62.0000],[3.0000,2.0000],true),UvRect::from_signed([7.0000,62.0000],[3.0000,2.0000],true),UvRect::from_signed([2.0000,62.0000],[1.0000,2.0000],true),UvRect::from_signed([6.0000,62.0000],[1.0000,2.0000],true)] },
UvReference { source: "goat", bone: "head", cube: 2, nominal: [64,64], faces: [UvRect::from_signed([3.0000,61.0000],[3.0000,1.0000],false),UvRect::from_signed([6.0000,61.0000],[3.0000,1.0000],false),UvRect::from_signed([3.0000,62.0000],[3.0000,2.0000],false),UvRect::from_signed([7.0000,62.0000],[3.0000,2.0000],false),UvRect::from_signed([2.0000,62.0000],[1.0000,2.0000],false),UvRect::from_signed([6.0000,62.0000],[1.0000,2.0000],false)] },
UvReference { source: "goat", bone: "head", cube: 3, nominal: [64,64], faces: [UvRect::from_signed([28.0000,52.0000],[0.0000,5.0000],false),UvRect::from_signed([28.0000,52.0000],[0.0000,5.0000],false),UvRect::from_signed([28.0000,57.0000],[0.0000,7.0000],false),UvRect::from_signed([33.0000,57.0000],[0.0000,7.0000],false),UvRect::from_signed([23.0000,57.0000],[5.0000,7.0000],false),UvRect::from_signed([28.0000,57.0000],[5.0000,7.0000],false)] },
UvReference { source: "goat", bone: "right_horn", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([14.0000,55.0000],[2.0000,2.0000],false),UvRect::from_signed([16.0000,55.0000],[2.0000,2.0000],false),UvRect::from_signed([14.0000,57.0000],[2.0000,7.0000],false),UvRect::from_signed([18.0000,57.0000],[2.0000,7.0000],false),UvRect::from_signed([12.0000,57.0000],[2.0000,7.0000],false),UvRect::from_signed([16.0000,57.0000],[2.0000,7.0000],false)] },
UvReference { source: "goat", bone: "left_horn", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([14.0000,55.0000],[2.0000,2.0000],false),UvRect::from_signed([16.0000,55.0000],[2.0000,2.0000],false),UvRect::from_signed([14.0000,57.0000],[2.0000,7.0000],false),UvRect::from_signed([18.0000,57.0000],[2.0000,7.0000],false),UvRect::from_signed([12.0000,57.0000],[2.0000,7.0000],false),UvRect::from_signed([16.0000,57.0000],[2.0000,7.0000],false)] },
UvReference { source: "horse_v3", bone: "Body", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([22.0000,32.0000],[10.0000,22.0000],false),UvRect::from_signed([32.0000,32.0000],[10.0000,22.0000],false),UvRect::from_signed([22.0000,54.0000],[10.0000,10.0000],false),UvRect::from_signed([54.0000,54.0000],[10.0000,10.0000],false),UvRect::from_signed([0.0000,54.0000],[22.0000,10.0000],false),UvRect::from_signed([32.0000,54.0000],[22.0000,10.0000],false)] },
UvReference { source: "horse_v3", bone: "Tail", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([46.0000,36.0000],[3.0000,4.0000],false),UvRect::from_signed([49.0000,36.0000],[3.0000,4.0000],false),UvRect::from_signed([46.0000,40.0000],[3.0000,14.0000],false),UvRect::from_signed([53.0000,40.0000],[3.0000,14.0000],false),UvRect::from_signed([42.0000,40.0000],[4.0000,14.0000],false),UvRect::from_signed([49.0000,40.0000],[4.0000,14.0000],false)] },
UvReference { source: "horse_v3", bone: "LegBL", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([52.0000,21.0000],[4.0000,4.0000],true),UvRect::from_signed([56.0000,21.0000],[4.0000,4.0000],true),UvRect::from_signed([52.0000,25.0000],[4.0000,11.0000],true),UvRect::from_signed([60.0000,25.0000],[4.0000,11.0000],true),UvRect::from_signed([48.0000,25.0000],[4.0000,11.0000],true),UvRect::from_signed([56.0000,25.0000],[4.0000,11.0000],true)] },
UvReference { source: "horse_v3", bone: "LegBR", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([52.0000,21.0000],[4.0000,4.0000],false),UvRect::from_signed([56.0000,21.0000],[4.0000,4.0000],false),UvRect::from_signed([52.0000,25.0000],[4.0000,11.0000],false),UvRect::from_signed([60.0000,25.0000],[4.0000,11.0000],false),UvRect::from_signed([48.0000,25.0000],[4.0000,11.0000],false),UvRect::from_signed([56.0000,25.0000],[4.0000,11.0000],false)] },
UvReference { source: "horse_v3", bone: "LegFL", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([52.0000,21.0000],[4.0000,4.0000],true),UvRect::from_signed([56.0000,21.0000],[4.0000,4.0000],true),UvRect::from_signed([52.0000,25.0000],[4.0000,11.0000],true),UvRect::from_signed([60.0000,25.0000],[4.0000,11.0000],true),UvRect::from_signed([48.0000,25.0000],[4.0000,11.0000],true),UvRect::from_signed([56.0000,25.0000],[4.0000,11.0000],true)] },
UvReference { source: "horse_v3", bone: "LegFR", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([52.0000,21.0000],[4.0000,4.0000],false),UvRect::from_signed([56.0000,21.0000],[4.0000,4.0000],false),UvRect::from_signed([52.0000,25.0000],[4.0000,11.0000],false),UvRect::from_signed([60.0000,25.0000],[4.0000,11.0000],false),UvRect::from_signed([48.0000,25.0000],[4.0000,11.0000],false),UvRect::from_signed([56.0000,25.0000],[4.0000,11.0000],false)] },
UvReference { source: "horse_v3", bone: "Neck", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([7.0000,35.0000],[4.0000,7.0000],false),UvRect::from_signed([11.0000,35.0000],[4.0000,7.0000],false),UvRect::from_signed([7.0000,42.0000],[4.0000,12.0000],false),UvRect::from_signed([18.0000,42.0000],[4.0000,12.0000],false),UvRect::from_signed([0.0000,42.0000],[7.0000,12.0000],false),UvRect::from_signed([11.0000,42.0000],[7.0000,12.0000],false)] },
UvReference { source: "horse_v3", bone: "Head", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([7.0000,13.0000],[6.0000,7.0000],false),UvRect::from_signed([13.0000,13.0000],[6.0000,7.0000],false),UvRect::from_signed([7.0000,20.0000],[6.0000,5.0000],false),UvRect::from_signed([20.0000,20.0000],[6.0000,5.0000],false),UvRect::from_signed([0.0000,20.0000],[7.0000,5.0000],false),UvRect::from_signed([13.0000,20.0000],[7.0000,5.0000],false)] },
UvReference { source: "horse_v3", bone: "Muzzle", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([5.0000,25.0000],[4.0000,5.0000],false),UvRect::from_signed([9.0000,25.0000],[4.0000,5.0000],false),UvRect::from_signed([5.0000,30.0000],[4.0000,5.0000],false),UvRect::from_signed([14.0000,30.0000],[4.0000,5.0000],false),UvRect::from_signed([0.0000,30.0000],[5.0000,5.0000],false),UvRect::from_signed([9.0000,30.0000],[5.0000,5.0000],false)] },
UvReference { source: "horse_v3", bone: "EarL", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([20.0000,16.0000],[2.0000,1.0000],true),UvRect::from_signed([22.0000,16.0000],[2.0000,1.0000],true),UvRect::from_signed([20.0000,17.0000],[2.0000,3.0000],true),UvRect::from_signed([23.0000,17.0000],[2.0000,3.0000],true),UvRect::from_signed([19.0000,17.0000],[1.0000,3.0000],true),UvRect::from_signed([22.0000,17.0000],[1.0000,3.0000],true)] },
UvReference { source: "horse_v3", bone: "EarR", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([20.0000,16.0000],[2.0000,1.0000],false),UvRect::from_signed([22.0000,16.0000],[2.0000,1.0000],false),UvRect::from_signed([20.0000,17.0000],[2.0000,3.0000],false),UvRect::from_signed([23.0000,17.0000],[2.0000,3.0000],false),UvRect::from_signed([19.0000,17.0000],[1.0000,3.0000],false),UvRect::from_signed([22.0000,17.0000],[1.0000,3.0000],false)] },
UvReference { source: "horse_v3", bone: "MuleEarL", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([1.0000,12.0000],[2.0000,1.0000],true),UvRect::from_signed([3.0000,12.0000],[2.0000,1.0000],true),UvRect::from_signed([1.0000,13.0000],[2.0000,7.0000],true),UvRect::from_signed([4.0000,13.0000],[2.0000,7.0000],true),UvRect::from_signed([0.0000,13.0000],[1.0000,7.0000],true),UvRect::from_signed([3.0000,13.0000],[1.0000,7.0000],true)] },
UvReference { source: "horse_v3", bone: "MuleEarR", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([1.0000,12.0000],[2.0000,1.0000],false),UvRect::from_signed([3.0000,12.0000],[2.0000,1.0000],false),UvRect::from_signed([1.0000,13.0000],[2.0000,7.0000],false),UvRect::from_signed([4.0000,13.0000],[2.0000,7.0000],false),UvRect::from_signed([0.0000,13.0000],[1.0000,7.0000],false),UvRect::from_signed([3.0000,13.0000],[1.0000,7.0000],false)] },
UvReference { source: "horse_v3", bone: "ReinsL", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([48.0000,2.0000],[0.0000,16.0000],false),UvRect::from_signed([48.0000,2.0000],[0.0000,16.0000],false),UvRect::from_signed([48.0000,18.0000],[0.0000,3.0000],false),UvRect::from_signed([64.0000,18.0000],[0.0000,3.0000],false),UvRect::from_signed([32.0000,18.0000],[16.0000,3.0000],false),UvRect::from_signed([48.0000,18.0000],[16.0000,3.0000],false)] },
UvReference { source: "horse_v3", bone: "ReinsR", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([48.0000,2.0000],[0.0000,16.0000],false),UvRect::from_signed([48.0000,2.0000],[0.0000,16.0000],false),UvRect::from_signed([48.0000,18.0000],[0.0000,3.0000],false),UvRect::from_signed([64.0000,18.0000],[0.0000,3.0000],false),UvRect::from_signed([32.0000,18.0000],[16.0000,3.0000],false),UvRect::from_signed([48.0000,18.0000],[16.0000,3.0000],false)] },
UvReference { source: "horse_v3", bone: "Bridle", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([21.0000,0.0000],[4.0000,2.0000],false),UvRect::from_signed([25.0000,0.0000],[4.0000,2.0000],false),UvRect::from_signed([21.0000,2.0000],[4.0000,5.0000],false),UvRect::from_signed([27.0000,2.0000],[4.0000,5.0000],false),UvRect::from_signed([19.0000,2.0000],[2.0000,5.0000],false),UvRect::from_signed([25.0000,2.0000],[2.0000,5.0000],false)] },
UvReference { source: "horse_v3", bone: "Bridle", cube: 1, nominal: [64,64], faces: [UvRect::from_signed([7.0000,0.0000],[6.0000,7.0000],false),UvRect::from_signed([13.0000,0.0000],[6.0000,7.0000],false),UvRect::from_signed([7.0000,7.0000],[6.0000,5.0000],false),UvRect::from_signed([20.0000,7.0000],[6.0000,5.0000],false),UvRect::from_signed([0.0000,7.0000],[7.0000,5.0000],false),UvRect::from_signed([13.0000,7.0000],[7.0000,5.0000],false)] },
UvReference { source: "horse_v3", bone: "BitL", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([31.0000,5.0000],[1.0000,2.0000],false),UvRect::from_signed([32.0000,5.0000],[1.0000,2.0000],false),UvRect::from_signed([31.0000,7.0000],[1.0000,2.0000],false),UvRect::from_signed([34.0000,7.0000],[1.0000,2.0000],false),UvRect::from_signed([29.0000,7.0000],[2.0000,2.0000],false),UvRect::from_signed([32.0000,7.0000],[2.0000,2.0000],false)] },
UvReference { source: "horse_v3", bone: "BitR", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([31.0000,5.0000],[1.0000,2.0000],false),UvRect::from_signed([32.0000,5.0000],[1.0000,2.0000],false),UvRect::from_signed([31.0000,7.0000],[1.0000,2.0000],false),UvRect::from_signed([34.0000,7.0000],[1.0000,2.0000],false),UvRect::from_signed([29.0000,7.0000],[2.0000,2.0000],false),UvRect::from_signed([32.0000,7.0000],[2.0000,2.0000],false)] },
UvReference { source: "horse_v3", bone: "Mane", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([58.0000,36.0000],[2.0000,2.0000],false),UvRect::from_signed([60.0000,36.0000],[2.0000,2.0000],false),UvRect::from_signed([58.0000,38.0000],[2.0000,16.0000],false),UvRect::from_signed([62.0000,38.0000],[2.0000,16.0000],false),UvRect::from_signed([56.0000,38.0000],[2.0000,16.0000],false),UvRect::from_signed([60.0000,38.0000],[2.0000,16.0000],false)] },
UvReference { source: "horse_v3", bone: "BagL", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([29.0000,21.0000],[8.0000,3.0000],true),UvRect::from_signed([37.0000,21.0000],[8.0000,3.0000],true),UvRect::from_signed([29.0000,24.0000],[8.0000,8.0000],true),UvRect::from_signed([40.0000,24.0000],[8.0000,8.0000],true),UvRect::from_signed([26.0000,24.0000],[3.0000,8.0000],true),UvRect::from_signed([37.0000,24.0000],[3.0000,8.0000],true)] },
UvReference { source: "horse_v3", bone: "BagR", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([29.0000,21.0000],[8.0000,3.0000],false),UvRect::from_signed([37.0000,21.0000],[8.0000,3.0000],false),UvRect::from_signed([29.0000,24.0000],[8.0000,8.0000],false),UvRect::from_signed([40.0000,24.0000],[8.0000,8.0000],false),UvRect::from_signed([26.0000,24.0000],[3.0000,8.0000],false),UvRect::from_signed([37.0000,24.0000],[3.0000,8.0000],false)] },
UvReference { source: "horse_v3", bone: "Saddle", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([35.0000,0.0000],[10.0000,9.0000],false),UvRect::from_signed([45.0000,0.0000],[10.0000,9.0000],false),UvRect::from_signed([35.0000,9.0000],[10.0000,9.0000],false),UvRect::from_signed([54.0000,9.0000],[10.0000,9.0000],false),UvRect::from_signed([26.0000,9.0000],[9.0000,9.0000],false),UvRect::from_signed([45.0000,9.0000],[9.0000,9.0000],false)] },
UvReference { source: "horse_v2", bone: "Body", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([22.0000,32.0000],[10.0000,22.0000],false),UvRect::from_signed([32.0000,32.0000],[10.0000,22.0000],false),UvRect::from_signed([22.0000,54.0000],[10.0000,10.0000],false),UvRect::from_signed([54.0000,54.0000],[10.0000,10.0000],false),UvRect::from_signed([0.0000,54.0000],[22.0000,10.0000],false),UvRect::from_signed([32.0000,54.0000],[22.0000,10.0000],false)] },
UvReference { source: "horse_v2", bone: "TailA", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([46.0000,36.0000],[3.0000,4.0000],false),UvRect::from_signed([49.0000,36.0000],[3.0000,4.0000],false),UvRect::from_signed([46.0000,40.0000],[3.0000,14.0000],false),UvRect::from_signed([53.0000,40.0000],[3.0000,14.0000],false),UvRect::from_signed([42.0000,40.0000],[4.0000,14.0000],false),UvRect::from_signed([49.0000,40.0000],[4.0000,14.0000],false)] },
UvReference { source: "horse_v2", bone: "Leg1A", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([52.0000,21.0000],[4.0000,4.0000],true),UvRect::from_signed([56.0000,21.0000],[4.0000,4.0000],true),UvRect::from_signed([52.0000,25.0000],[4.0000,11.0000],true),UvRect::from_signed([60.0000,25.0000],[4.0000,11.0000],true),UvRect::from_signed([48.0000,25.0000],[4.0000,11.0000],true),UvRect::from_signed([56.0000,25.0000],[4.0000,11.0000],true)] },
UvReference { source: "horse_v2", bone: "Leg2A", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([52.0000,21.0000],[4.0000,4.0000],false),UvRect::from_signed([56.0000,21.0000],[4.0000,4.0000],false),UvRect::from_signed([52.0000,25.0000],[4.0000,11.0000],false),UvRect::from_signed([60.0000,25.0000],[4.0000,11.0000],false),UvRect::from_signed([48.0000,25.0000],[4.0000,11.0000],false),UvRect::from_signed([56.0000,25.0000],[4.0000,11.0000],false)] },
UvReference { source: "horse_v2", bone: "Leg3A", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([52.0000,21.0000],[4.0000,4.0000],true),UvRect::from_signed([56.0000,21.0000],[4.0000,4.0000],true),UvRect::from_signed([52.0000,25.0000],[4.0000,11.0000],true),UvRect::from_signed([60.0000,25.0000],[4.0000,11.0000],true),UvRect::from_signed([48.0000,25.0000],[4.0000,11.0000],true),UvRect::from_signed([56.0000,25.0000],[4.0000,11.0000],true)] },
UvReference { source: "horse_v2", bone: "Leg4A", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([52.0000,21.0000],[4.0000,4.0000],false),UvRect::from_signed([56.0000,21.0000],[4.0000,4.0000],false),UvRect::from_signed([52.0000,25.0000],[4.0000,11.0000],false),UvRect::from_signed([60.0000,25.0000],[4.0000,11.0000],false),UvRect::from_signed([48.0000,25.0000],[4.0000,11.0000],false),UvRect::from_signed([56.0000,25.0000],[4.0000,11.0000],false)] },
UvReference { source: "horse_v2", bone: "Head", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([7.0000,13.0000],[6.0000,7.0000],false),UvRect::from_signed([13.0000,13.0000],[6.0000,7.0000],false),UvRect::from_signed([7.0000,20.0000],[6.0000,5.0000],false),UvRect::from_signed([20.0000,20.0000],[6.0000,5.0000],false),UvRect::from_signed([0.0000,20.0000],[7.0000,5.0000],false),UvRect::from_signed([13.0000,20.0000],[7.0000,5.0000],false)] },
UvReference { source: "horse_v2", bone: "UMouth", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([5.0000,25.0000],[4.0000,5.0000],false),UvRect::from_signed([9.0000,25.0000],[4.0000,5.0000],false),UvRect::from_signed([5.0000,30.0000],[4.0000,5.0000],false),UvRect::from_signed([14.0000,30.0000],[4.0000,5.0000],false),UvRect::from_signed([0.0000,30.0000],[5.0000,5.0000],false),UvRect::from_signed([9.0000,30.0000],[5.0000,5.0000],false)] },
UvReference { source: "horse_v2", bone: "Ear1", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([20.0000,16.0000],[2.0000,1.0000],true),UvRect::from_signed([22.0000,16.0000],[2.0000,1.0000],true),UvRect::from_signed([20.0000,17.0000],[2.0000,3.0000],true),UvRect::from_signed([23.0000,17.0000],[2.0000,3.0000],true),UvRect::from_signed([19.0000,17.0000],[1.0000,3.0000],true),UvRect::from_signed([22.0000,17.0000],[1.0000,3.0000],true)] },
UvReference { source: "horse_v2", bone: "Ear2", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([20.0000,16.0000],[2.0000,1.0000],false),UvRect::from_signed([22.0000,16.0000],[2.0000,1.0000],false),UvRect::from_signed([20.0000,17.0000],[2.0000,3.0000],false),UvRect::from_signed([23.0000,17.0000],[2.0000,3.0000],false),UvRect::from_signed([19.0000,17.0000],[1.0000,3.0000],false),UvRect::from_signed([22.0000,17.0000],[1.0000,3.0000],false)] },
UvReference { source: "horse_v2", bone: "MuleEarL", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([1.0000,12.0000],[2.0000,1.0000],true),UvRect::from_signed([3.0000,12.0000],[2.0000,1.0000],true),UvRect::from_signed([1.0000,13.0000],[2.0000,7.0000],true),UvRect::from_signed([4.0000,13.0000],[2.0000,7.0000],true),UvRect::from_signed([0.0000,13.0000],[1.0000,7.0000],true),UvRect::from_signed([3.0000,13.0000],[1.0000,7.0000],true)] },
UvReference { source: "horse_v2", bone: "MuleEarR", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([1.0000,12.0000],[2.0000,1.0000],false),UvRect::from_signed([3.0000,12.0000],[2.0000,1.0000],false),UvRect::from_signed([1.0000,13.0000],[2.0000,7.0000],false),UvRect::from_signed([4.0000,13.0000],[2.0000,7.0000],false),UvRect::from_signed([0.0000,13.0000],[1.0000,7.0000],false),UvRect::from_signed([3.0000,13.0000],[1.0000,7.0000],false)] },
UvReference { source: "horse_v2", bone: "Neck", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([7.0000,35.0000],[4.0000,7.0000],false),UvRect::from_signed([11.0000,35.0000],[4.0000,7.0000],false),UvRect::from_signed([7.0000,42.0000],[4.0000,12.0000],false),UvRect::from_signed([18.0000,42.0000],[4.0000,12.0000],false),UvRect::from_signed([0.0000,42.0000],[7.0000,12.0000],false),UvRect::from_signed([11.0000,42.0000],[7.0000,12.0000],false)] },
UvReference { source: "horse_v2", bone: "Bag1", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([29.0000,21.0000],[8.0000,3.0000],false),UvRect::from_signed([37.0000,21.0000],[8.0000,3.0000],false),UvRect::from_signed([29.0000,24.0000],[8.0000,8.0000],false),UvRect::from_signed([40.0000,24.0000],[8.0000,8.0000],false),UvRect::from_signed([26.0000,24.0000],[3.0000,8.0000],false),UvRect::from_signed([37.0000,24.0000],[3.0000,8.0000],false)] },
UvReference { source: "horse_v2", bone: "Bag2", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([29.0000,21.0000],[8.0000,3.0000],true),UvRect::from_signed([37.0000,21.0000],[8.0000,3.0000],true),UvRect::from_signed([29.0000,24.0000],[8.0000,8.0000],true),UvRect::from_signed([40.0000,24.0000],[8.0000,8.0000],true),UvRect::from_signed([26.0000,24.0000],[3.0000,8.0000],true),UvRect::from_signed([37.0000,24.0000],[3.0000,8.0000],true)] },
UvReference { source: "horse_v2", bone: "Saddle", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([35.0000,0.0000],[10.0000,9.0000],false),UvRect::from_signed([45.0000,0.0000],[10.0000,9.0000],false),UvRect::from_signed([35.0000,9.0000],[10.0000,9.0000],false),UvRect::from_signed([54.0000,9.0000],[10.0000,9.0000],false),UvRect::from_signed([26.0000,9.0000],[9.0000,9.0000],false),UvRect::from_signed([45.0000,9.0000],[9.0000,9.0000],false)] },
UvReference { source: "horse_v2", bone: "SaddleMouthL", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([31.0000,5.0000],[1.0000,2.0000],false),UvRect::from_signed([32.0000,5.0000],[1.0000,2.0000],false),UvRect::from_signed([31.0000,7.0000],[1.0000,2.0000],false),UvRect::from_signed([34.0000,7.0000],[1.0000,2.0000],false),UvRect::from_signed([29.0000,7.0000],[2.0000,2.0000],false),UvRect::from_signed([32.0000,7.0000],[2.0000,2.0000],false)] },
UvReference { source: "horse_v2", bone: "SaddleMouthR", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([31.0000,5.0000],[1.0000,2.0000],false),UvRect::from_signed([32.0000,5.0000],[1.0000,2.0000],false),UvRect::from_signed([31.0000,7.0000],[1.0000,2.0000],false),UvRect::from_signed([34.0000,7.0000],[1.0000,2.0000],false),UvRect::from_signed([29.0000,7.0000],[2.0000,2.0000],false),UvRect::from_signed([32.0000,7.0000],[2.0000,2.0000],false)] },
UvReference { source: "horse_v2", bone: "SaddleMouthLine", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([48.0000,2.0000],[0.0000,16.0000],false),UvRect::from_signed([48.0000,2.0000],[0.0000,16.0000],false),UvRect::from_signed([48.0000,18.0000],[0.0000,3.0000],false),UvRect::from_signed([64.0000,18.0000],[0.0000,3.0000],false),UvRect::from_signed([32.0000,18.0000],[16.0000,3.0000],false),UvRect::from_signed([48.0000,18.0000],[16.0000,3.0000],false)] },
UvReference { source: "horse_v2", bone: "SaddleMouthLineR", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([48.0000,2.0000],[0.0000,16.0000],false),UvRect::from_signed([48.0000,2.0000],[0.0000,16.0000],false),UvRect::from_signed([48.0000,18.0000],[0.0000,3.0000],false),UvRect::from_signed([64.0000,18.0000],[0.0000,3.0000],false),UvRect::from_signed([32.0000,18.0000],[16.0000,3.0000],false),UvRect::from_signed([48.0000,18.0000],[16.0000,3.0000],false)] },
UvReference { source: "horse_v2", bone: "Mane", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([58.0000,36.0000],[2.0000,2.0000],false),UvRect::from_signed([60.0000,36.0000],[2.0000,2.0000],false),UvRect::from_signed([58.0000,38.0000],[2.0000,16.0000],false),UvRect::from_signed([62.0000,38.0000],[2.0000,16.0000],false),UvRect::from_signed([56.0000,38.0000],[2.0000,16.0000],false),UvRect::from_signed([60.0000,38.0000],[2.0000,16.0000],false)] },
UvReference { source: "horse_v2", bone: "HeadSaddle", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([21.0000,0.0000],[4.0000,2.0000],false),UvRect::from_signed([25.0000,0.0000],[4.0000,2.0000],false),UvRect::from_signed([21.0000,2.0000],[4.0000,5.0000],false),UvRect::from_signed([27.0000,2.0000],[4.0000,5.0000],false),UvRect::from_signed([19.0000,2.0000],[2.0000,5.0000],false),UvRect::from_signed([25.0000,2.0000],[2.0000,5.0000],false)] },
UvReference { source: "horse_v2", bone: "HeadSaddle", cube: 1, nominal: [64,64], faces: [UvRect::from_signed([7.0000,0.0000],[6.0000,7.0000],false),UvRect::from_signed([13.0000,0.0000],[6.0000,7.0000],false),UvRect::from_signed([7.0000,7.0000],[6.0000,5.0000],false),UvRect::from_signed([20.0000,7.0000],[6.0000,5.0000],false),UvRect::from_signed([0.0000,7.0000],[7.0000,5.0000],false),UvRect::from_signed([13.0000,7.0000],[7.0000,5.0000],false)] },
UvReference { source: "husk", bone: "body", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([20.0000,16.0000],[8.0000,4.0000],false),UvRect::from_signed([28.0000,16.0000],[8.0000,4.0000],false),UvRect::from_signed([20.0000,20.0000],[8.0000,12.0000],false),UvRect::from_signed([32.0000,20.0000],[8.0000,12.0000],false),UvRect::from_signed([16.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([28.0000,20.0000],[4.0000,12.0000],false)] },
UvReference { source: "husk", bone: "head", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([8.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([16.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([8.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([24.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([0.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([16.0000,8.0000],[8.0000,8.0000],false)] },
UvReference { source: "husk", bone: "hat", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([40.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([48.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([40.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([56.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([32.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([48.0000,8.0000],[8.0000,8.0000],false)] },
UvReference { source: "husk", bone: "rightArm", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([44.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([48.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([44.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([52.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([40.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([48.0000,20.0000],[4.0000,12.0000],false)] },
UvReference { source: "husk", bone: "leftArm", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([44.0000,16.0000],[4.0000,4.0000],true),UvRect::from_signed([48.0000,16.0000],[4.0000,4.0000],true),UvRect::from_signed([44.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([52.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([40.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([48.0000,20.0000],[4.0000,12.0000],true)] },
UvReference { source: "husk", bone: "rightLeg", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([4.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([12.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([0.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([8.0000,20.0000],[4.0000,12.0000],false)] },
UvReference { source: "husk", bone: "leftLeg", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],true),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],true),UvRect::from_signed([4.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([12.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([0.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([8.0000,20.0000],[4.0000,12.0000],true)] },
UvReference { source: "iron_golem", bone: "body", cube: 0, nominal: [128,128], faces: [UvRect::from_signed([11.0000,40.0000],[18.0000,11.0000],false),UvRect::from_signed([29.0000,40.0000],[18.0000,11.0000],false),UvRect::from_signed([11.0000,51.0000],[18.0000,12.0000],false),UvRect::from_signed([40.0000,51.0000],[18.0000,12.0000],false),UvRect::from_signed([0.0000,51.0000],[11.0000,12.0000],false),UvRect::from_signed([29.0000,51.0000],[11.0000,12.0000],false)] },
UvReference { source: "iron_golem", bone: "body", cube: 1, nominal: [128,128], faces: [UvRect::from_signed([6.0000,70.0000],[9.0000,6.0000],false),UvRect::from_signed([15.0000,70.0000],[9.0000,6.0000],false),UvRect::from_signed([6.0000,76.0000],[9.0000,5.0000],false),UvRect::from_signed([21.0000,76.0000],[9.0000,5.0000],false),UvRect::from_signed([0.0000,76.0000],[6.0000,5.0000],false),UvRect::from_signed([15.0000,76.0000],[6.0000,5.0000],false)] },
UvReference { source: "iron_golem", bone: "head", cube: 0, nominal: [128,128], faces: [UvRect::from_signed([8.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([16.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([8.0000,8.0000],[8.0000,10.0000],false),UvRect::from_signed([24.0000,8.0000],[8.0000,10.0000],false),UvRect::from_signed([0.0000,8.0000],[8.0000,10.0000],false),UvRect::from_signed([16.0000,8.0000],[8.0000,10.0000],false)] },
UvReference { source: "iron_golem", bone: "head", cube: 1, nominal: [128,128], faces: [UvRect::from_signed([26.0000,0.0000],[2.0000,2.0000],false),UvRect::from_signed([28.0000,0.0000],[2.0000,2.0000],false),UvRect::from_signed([26.0000,2.0000],[2.0000,4.0000],false),UvRect::from_signed([30.0000,2.0000],[2.0000,4.0000],false),UvRect::from_signed([24.0000,2.0000],[2.0000,4.0000],false),UvRect::from_signed([28.0000,2.0000],[2.0000,4.0000],false)] },
UvReference { source: "iron_golem", bone: "arm0", cube: 0, nominal: [128,128], faces: [UvRect::from_signed([66.0000,21.0000],[4.0000,6.0000],false),UvRect::from_signed([70.0000,21.0000],[4.0000,6.0000],false),UvRect::from_signed([66.0000,27.0000],[4.0000,30.0000],false),UvRect::from_signed([76.0000,27.0000],[4.0000,30.0000],false),UvRect::from_signed([60.0000,27.0000],[6.0000,30.0000],false),UvRect::from_signed([70.0000,27.0000],[6.0000,30.0000],false)] },
UvReference { source: "iron_golem", bone: "arm1", cube: 0, nominal: [128,128], faces: [UvRect::from_signed([66.0000,58.0000],[4.0000,6.0000],false),UvRect::from_signed([70.0000,58.0000],[4.0000,6.0000],false),UvRect::from_signed([66.0000,64.0000],[4.0000,30.0000],false),UvRect::from_signed([76.0000,64.0000],[4.0000,30.0000],false),UvRect::from_signed([60.0000,64.0000],[6.0000,30.0000],false),UvRect::from_signed([70.0000,64.0000],[6.0000,30.0000],false)] },
UvReference { source: "iron_golem", bone: "leg0", cube: 0, nominal: [128,128], faces: [UvRect::from_signed([42.0000,0.0000],[6.0000,5.0000],false),UvRect::from_signed([48.0000,0.0000],[6.0000,5.0000],false),UvRect::from_signed([42.0000,5.0000],[6.0000,16.0000],false),UvRect::from_signed([53.0000,5.0000],[6.0000,16.0000],false),UvRect::from_signed([37.0000,5.0000],[5.0000,16.0000],false),UvRect::from_signed([48.0000,5.0000],[5.0000,16.0000],false)] },
UvReference { source: "iron_golem", bone: "leg1", cube: 0, nominal: [128,128], faces: [UvRect::from_signed([65.0000,0.0000],[6.0000,5.0000],true),UvRect::from_signed([71.0000,0.0000],[6.0000,5.0000],true),UvRect::from_signed([65.0000,5.0000],[6.0000,16.0000],true),UvRect::from_signed([76.0000,5.0000],[6.0000,16.0000],true),UvRect::from_signed([60.0000,5.0000],[5.0000,16.0000],true),UvRect::from_signed([71.0000,5.0000],[5.0000,16.0000],true)] },
UvReference { source: "llama", bone: "head", cube: 0, nominal: [128,64], faces: [UvRect::from_signed([9.0000,0.0000],[4.0000,9.0000],false),UvRect::from_signed([13.0000,0.0000],[4.0000,9.0000],false),UvRect::from_signed([9.0000,9.0000],[4.0000,4.0000],false),UvRect::from_signed([22.0000,9.0000],[4.0000,4.0000],false),UvRect::from_signed([0.0000,9.0000],[9.0000,4.0000],false),UvRect::from_signed([13.0000,9.0000],[9.0000,4.0000],false)] },
UvReference { source: "llama", bone: "head", cube: 1, nominal: [128,64], faces: [UvRect::from_signed([6.0000,14.0000],[8.0000,6.0000],false),UvRect::from_signed([14.0000,14.0000],[8.0000,6.0000],false),UvRect::from_signed([6.0000,20.0000],[8.0000,18.0000],false),UvRect::from_signed([20.0000,20.0000],[8.0000,18.0000],false),UvRect::from_signed([0.0000,20.0000],[6.0000,18.0000],false),UvRect::from_signed([14.0000,20.0000],[6.0000,18.0000],false)] },
UvReference { source: "llama", bone: "head", cube: 2, nominal: [128,64], faces: [UvRect::from_signed([19.0000,0.0000],[3.0000,2.0000],false),UvRect::from_signed([22.0000,0.0000],[3.0000,2.0000],false),UvRect::from_signed([19.0000,2.0000],[3.0000,3.0000],false),UvRect::from_signed([24.0000,2.0000],[3.0000,3.0000],false),UvRect::from_signed([17.0000,2.0000],[2.0000,3.0000],false),UvRect::from_signed([22.0000,2.0000],[2.0000,3.0000],false)] },
UvReference { source: "llama", bone: "head", cube: 3, nominal: [128,64], faces: [UvRect::from_signed([19.0000,0.0000],[3.0000,2.0000],false),UvRect::from_signed([22.0000,0.0000],[3.0000,2.0000],false),UvRect::from_signed([19.0000,2.0000],[3.0000,3.0000],false),UvRect::from_signed([24.0000,2.0000],[3.0000,3.0000],false),UvRect::from_signed([17.0000,2.0000],[2.0000,3.0000],false),UvRect::from_signed([22.0000,2.0000],[2.0000,3.0000],false)] },
UvReference { source: "llama", bone: "chest1", cube: 0, nominal: [128,64], faces: [UvRect::from_signed([48.0000,28.0000],[8.0000,3.0000],false),UvRect::from_signed([56.0000,28.0000],[8.0000,3.0000],false),UvRect::from_signed([48.0000,31.0000],[8.0000,8.0000],false),UvRect::from_signed([59.0000,31.0000],[8.0000,8.0000],false),UvRect::from_signed([45.0000,31.0000],[3.0000,8.0000],false),UvRect::from_signed([56.0000,31.0000],[3.0000,8.0000],false)] },
UvReference { source: "llama", bone: "chest2", cube: 0, nominal: [128,64], faces: [UvRect::from_signed([48.0000,41.0000],[8.0000,3.0000],false),UvRect::from_signed([56.0000,41.0000],[8.0000,3.0000],false),UvRect::from_signed([48.0000,44.0000],[8.0000,8.0000],false),UvRect::from_signed([59.0000,44.0000],[8.0000,8.0000],false),UvRect::from_signed([45.0000,44.0000],[3.0000,8.0000],false),UvRect::from_signed([56.0000,44.0000],[3.0000,8.0000],false)] },
UvReference { source: "llama", bone: "body", cube: 0, nominal: [128,64], faces: [UvRect::from_signed([39.0000,0.0000],[12.0000,10.0000],false),UvRect::from_signed([51.0000,0.0000],[12.0000,10.0000],false),UvRect::from_signed([39.0000,10.0000],[12.0000,18.0000],false),UvRect::from_signed([61.0000,10.0000],[12.0000,18.0000],false),UvRect::from_signed([29.0000,10.0000],[10.0000,18.0000],false),UvRect::from_signed([51.0000,10.0000],[10.0000,18.0000],false)] },
UvReference { source: "llama", bone: "leg0", cube: 0, nominal: [128,64], faces: [UvRect::from_signed([33.0000,29.0000],[4.0000,4.0000],false),UvRect::from_signed([37.0000,29.0000],[4.0000,4.0000],false),UvRect::from_signed([33.0000,33.0000],[4.0000,14.0000],false),UvRect::from_signed([41.0000,33.0000],[4.0000,14.0000],false),UvRect::from_signed([29.0000,33.0000],[4.0000,14.0000],false),UvRect::from_signed([37.0000,33.0000],[4.0000,14.0000],false)] },
UvReference { source: "llama", bone: "leg1", cube: 0, nominal: [128,64], faces: [UvRect::from_signed([33.0000,29.0000],[4.0000,4.0000],false),UvRect::from_signed([37.0000,29.0000],[4.0000,4.0000],false),UvRect::from_signed([33.0000,33.0000],[4.0000,14.0000],false),UvRect::from_signed([41.0000,33.0000],[4.0000,14.0000],false),UvRect::from_signed([29.0000,33.0000],[4.0000,14.0000],false),UvRect::from_signed([37.0000,33.0000],[4.0000,14.0000],false)] },
UvReference { source: "llama", bone: "leg2", cube: 0, nominal: [128,64], faces: [UvRect::from_signed([33.0000,29.0000],[4.0000,4.0000],false),UvRect::from_signed([37.0000,29.0000],[4.0000,4.0000],false),UvRect::from_signed([33.0000,33.0000],[4.0000,14.0000],false),UvRect::from_signed([41.0000,33.0000],[4.0000,14.0000],false),UvRect::from_signed([29.0000,33.0000],[4.0000,14.0000],false),UvRect::from_signed([37.0000,33.0000],[4.0000,14.0000],false)] },
UvReference { source: "llama", bone: "leg3", cube: 0, nominal: [128,64], faces: [UvRect::from_signed([33.0000,29.0000],[4.0000,4.0000],false),UvRect::from_signed([37.0000,29.0000],[4.0000,4.0000],false),UvRect::from_signed([33.0000,33.0000],[4.0000,14.0000],false),UvRect::from_signed([41.0000,33.0000],[4.0000,14.0000],false),UvRect::from_signed([29.0000,33.0000],[4.0000,14.0000],false),UvRect::from_signed([37.0000,33.0000],[4.0000,14.0000],false)] },
UvReference { source: "mooshroom", bone: "body", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([28.0000,4.0000],[12.0000,10.0000],false),UvRect::from_signed([40.0000,4.0000],[12.0000,10.0000],false),UvRect::from_signed([28.0000,14.0000],[12.0000,18.0000],false),UvRect::from_signed([50.0000,14.0000],[12.0000,18.0000],false),UvRect::from_signed([18.0000,14.0000],[10.0000,18.0000],false),UvRect::from_signed([40.0000,14.0000],[10.0000,18.0000],false)] },
UvReference { source: "mooshroom", bone: "body", cube: 1, nominal: [64,32], faces: [UvRect::from_signed([53.0000,0.0000],[4.0000,1.0000],false),UvRect::from_signed([57.0000,0.0000],[4.0000,1.0000],false),UvRect::from_signed([53.0000,1.0000],[4.0000,6.0000],false),UvRect::from_signed([58.0000,1.0000],[4.0000,6.0000],false),UvRect::from_signed([52.0000,1.0000],[1.0000,6.0000],false),UvRect::from_signed([57.0000,1.0000],[1.0000,6.0000],false)] },
UvReference { source: "mooshroom", bone: "head", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([6.0000,0.0000],[8.0000,6.0000],false),UvRect::from_signed([14.0000,0.0000],[8.0000,6.0000],false),UvRect::from_signed([6.0000,6.0000],[8.0000,8.0000],false),UvRect::from_signed([20.0000,6.0000],[8.0000,8.0000],false),UvRect::from_signed([0.0000,6.0000],[6.0000,8.0000],false),UvRect::from_signed([14.0000,6.0000],[6.0000,8.0000],false)] },
UvReference { source: "mooshroom", bone: "head", cube: 1, nominal: [64,32], faces: [UvRect::from_signed([23.0000,0.0000],[1.0000,1.0000],false),UvRect::from_signed([24.0000,0.0000],[1.0000,1.0000],false),UvRect::from_signed([23.0000,1.0000],[1.0000,3.0000],false),UvRect::from_signed([25.0000,1.0000],[1.0000,3.0000],false),UvRect::from_signed([22.0000,1.0000],[1.0000,3.0000],false),UvRect::from_signed([24.0000,1.0000],[1.0000,3.0000],false)] },
UvReference { source: "mooshroom", bone: "head", cube: 2, nominal: [64,32], faces: [UvRect::from_signed([23.0000,0.0000],[1.0000,1.0000],false),UvRect::from_signed([24.0000,0.0000],[1.0000,1.0000],false),UvRect::from_signed([23.0000,1.0000],[1.0000,3.0000],false),UvRect::from_signed([25.0000,1.0000],[1.0000,3.0000],false),UvRect::from_signed([22.0000,1.0000],[1.0000,3.0000],false),UvRect::from_signed([24.0000,1.0000],[1.0000,3.0000],false)] },
UvReference { source: "mooshroom", bone: "leg0", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([4.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([12.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([0.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([8.0000,20.0000],[4.0000,12.0000],false)] },
UvReference { source: "mooshroom", bone: "leg1", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],true),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],true),UvRect::from_signed([4.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([12.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([0.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([8.0000,20.0000],[4.0000,12.0000],true)] },
UvReference { source: "mooshroom", bone: "leg2", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([4.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([12.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([0.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([8.0000,20.0000],[4.0000,12.0000],false)] },
UvReference { source: "mooshroom", bone: "leg3", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],true),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],true),UvRect::from_signed([4.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([12.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([0.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([8.0000,20.0000],[4.0000,12.0000],true)] },
UvReference { source: "ocelot", bone: "head", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([5.0000,0.0000],[5.0000,5.0000],false),UvRect::from_signed([10.0000,0.0000],[5.0000,5.0000],false),UvRect::from_signed([5.0000,5.0000],[5.0000,4.0000],false),UvRect::from_signed([15.0000,5.0000],[5.0000,4.0000],false),UvRect::from_signed([0.0000,5.0000],[5.0000,4.0000],false),UvRect::from_signed([10.0000,5.0000],[5.0000,4.0000],false)] },
UvReference { source: "ocelot", bone: "head", cube: 1, nominal: [64,32], faces: [UvRect::from_signed([2.0000,24.0000],[3.0000,2.0000],false),UvRect::from_signed([5.0000,24.0000],[3.0000,2.0000],false),UvRect::from_signed([2.0000,26.0000],[3.0000,2.0000],false),UvRect::from_signed([7.0000,26.0000],[3.0000,2.0000],false),UvRect::from_signed([0.0000,26.0000],[2.0000,2.0000],false),UvRect::from_signed([5.0000,26.0000],[2.0000,2.0000],false)] },
UvReference { source: "ocelot", bone: "head", cube: 2, nominal: [64,32], faces: [UvRect::from_signed([2.0000,10.0000],[1.0000,2.0000],false),UvRect::from_signed([3.0000,10.0000],[1.0000,2.0000],false),UvRect::from_signed([2.0000,12.0000],[1.0000,1.0000],false),UvRect::from_signed([5.0000,12.0000],[1.0000,1.0000],false),UvRect::from_signed([0.0000,12.0000],[2.0000,1.0000],false),UvRect::from_signed([3.0000,12.0000],[2.0000,1.0000],false)] },
UvReference { source: "ocelot", bone: "head", cube: 3, nominal: [64,32], faces: [UvRect::from_signed([8.0000,10.0000],[1.0000,2.0000],false),UvRect::from_signed([9.0000,10.0000],[1.0000,2.0000],false),UvRect::from_signed([8.0000,12.0000],[1.0000,1.0000],false),UvRect::from_signed([11.0000,12.0000],[1.0000,1.0000],false),UvRect::from_signed([6.0000,12.0000],[2.0000,1.0000],false),UvRect::from_signed([9.0000,12.0000],[2.0000,1.0000],false)] },
UvReference { source: "ocelot", bone: "body", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([26.0000,0.0000],[4.0000,6.0000],false),UvRect::from_signed([30.0000,0.0000],[4.0000,6.0000],false),UvRect::from_signed([26.0000,6.0000],[4.0000,16.0000],false),UvRect::from_signed([36.0000,6.0000],[4.0000,16.0000],false),UvRect::from_signed([20.0000,6.0000],[6.0000,16.0000],false),UvRect::from_signed([30.0000,6.0000],[6.0000,16.0000],false)] },
UvReference { source: "ocelot", bone: "tail1", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([1.0000,15.0000],[1.0000,1.0000],false),UvRect::from_signed([2.0000,15.0000],[1.0000,1.0000],false),UvRect::from_signed([1.0000,16.0000],[1.0000,8.0000],false),UvRect::from_signed([3.0000,16.0000],[1.0000,8.0000],false),UvRect::from_signed([0.0000,16.0000],[1.0000,8.0000],false),UvRect::from_signed([2.0000,16.0000],[1.0000,8.0000],false)] },
UvReference { source: "ocelot", bone: "tail2", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([5.0000,15.0000],[1.0000,1.0000],false),UvRect::from_signed([6.0000,15.0000],[1.0000,1.0000],false),UvRect::from_signed([5.0000,16.0000],[1.0000,8.0000],false),UvRect::from_signed([7.0000,16.0000],[1.0000,8.0000],false),UvRect::from_signed([4.0000,16.0000],[1.0000,8.0000],false),UvRect::from_signed([6.0000,16.0000],[1.0000,8.0000],false)] },
UvReference { source: "ocelot", bone: "backLegL", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([10.0000,13.0000],[2.0000,2.0000],false),UvRect::from_signed([12.0000,13.0000],[2.0000,2.0000],false),UvRect::from_signed([10.0000,15.0000],[2.0000,6.0000],false),UvRect::from_signed([14.0000,15.0000],[2.0000,6.0000],false),UvRect::from_signed([8.0000,15.0000],[2.0000,6.0000],false),UvRect::from_signed([12.0000,15.0000],[2.0000,6.0000],false)] },
UvReference { source: "ocelot", bone: "backLegR", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([10.0000,13.0000],[2.0000,2.0000],false),UvRect::from_signed([12.0000,13.0000],[2.0000,2.0000],false),UvRect::from_signed([10.0000,15.0000],[2.0000,6.0000],false),UvRect::from_signed([14.0000,15.0000],[2.0000,6.0000],false),UvRect::from_signed([8.0000,15.0000],[2.0000,6.0000],false),UvRect::from_signed([12.0000,15.0000],[2.0000,6.0000],false)] },
UvReference { source: "ocelot", bone: "frontLegL", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([42.0000,0.0000],[2.0000,2.0000],false),UvRect::from_signed([44.0000,0.0000],[2.0000,2.0000],false),UvRect::from_signed([42.0000,2.0000],[2.0000,10.0000],false),UvRect::from_signed([46.0000,2.0000],[2.0000,10.0000],false),UvRect::from_signed([40.0000,2.0000],[2.0000,10.0000],false),UvRect::from_signed([44.0000,2.0000],[2.0000,10.0000],false)] },
UvReference { source: "ocelot", bone: "frontLegR", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([42.0000,0.0000],[2.0000,2.0000],false),UvRect::from_signed([44.0000,0.0000],[2.0000,2.0000],false),UvRect::from_signed([42.0000,2.0000],[2.0000,10.0000],false),UvRect::from_signed([46.0000,2.0000],[2.0000,10.0000],false),UvRect::from_signed([40.0000,2.0000],[2.0000,10.0000],false),UvRect::from_signed([44.0000,2.0000],[2.0000,10.0000],false)] },
UvReference { source: "panda", bone: "head", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([9.0000,6.0000],[13.0000,9.0000],false),UvRect::from_signed([22.0000,6.0000],[13.0000,9.0000],false),UvRect::from_signed([9.0000,15.0000],[13.0000,10.0000],false),UvRect::from_signed([31.0000,15.0000],[13.0000,10.0000],false),UvRect::from_signed([0.0000,15.0000],[9.0000,10.0000],false),UvRect::from_signed([22.0000,15.0000],[9.0000,10.0000],false)] },
UvReference { source: "panda", bone: "head", cube: 1, nominal: [64,64], faces: [UvRect::from_signed([47.0000,16.0000],[7.0000,2.0000],false),UvRect::from_signed([54.0000,16.0000],[7.0000,2.0000],false),UvRect::from_signed([47.0000,18.0000],[7.0000,5.0000],false),UvRect::from_signed([56.0000,18.0000],[7.0000,5.0000],false),UvRect::from_signed([45.0000,18.0000],[2.0000,5.0000],false),UvRect::from_signed([54.0000,18.0000],[2.0000,5.0000],false)] },
UvReference { source: "panda", bone: "head", cube: 2, nominal: [64,64], faces: [UvRect::from_signed([53.0000,25.0000],[5.0000,1.0000],false),UvRect::from_signed([58.0000,25.0000],[5.0000,1.0000],false),UvRect::from_signed([53.0000,26.0000],[5.0000,4.0000],false),UvRect::from_signed([59.0000,26.0000],[5.0000,4.0000],false),UvRect::from_signed([52.0000,26.0000],[1.0000,4.0000],false),UvRect::from_signed([58.0000,26.0000],[1.0000,4.0000],false)] },
UvReference { source: "panda", bone: "head", cube: 3, nominal: [64,64], faces: [UvRect::from_signed([53.0000,25.0000],[5.0000,1.0000],false),UvRect::from_signed([58.0000,25.0000],[5.0000,1.0000],false),UvRect::from_signed([53.0000,26.0000],[5.0000,4.0000],false),UvRect::from_signed([59.0000,26.0000],[5.0000,4.0000],false),UvRect::from_signed([52.0000,26.0000],[1.0000,4.0000],false),UvRect::from_signed([58.0000,26.0000],[1.0000,4.0000],false)] },
UvReference { source: "panda", bone: "body", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([13.0000,25.0000],[19.0000,13.0000],false),UvRect::from_signed([32.0000,25.0000],[19.0000,13.0000],false),UvRect::from_signed([13.0000,38.0000],[19.0000,26.0000],false),UvRect::from_signed([45.0000,38.0000],[19.0000,26.0000],false),UvRect::from_signed([0.0000,38.0000],[13.0000,26.0000],false),UvRect::from_signed([32.0000,38.0000],[13.0000,26.0000],false)] },
UvReference { source: "panda", bone: "leg0", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([46.0000,0.0000],[6.0000,6.0000],false),UvRect::from_signed([52.0000,0.0000],[6.0000,6.0000],false),UvRect::from_signed([46.0000,6.0000],[6.0000,9.0000],false),UvRect::from_signed([58.0000,6.0000],[6.0000,9.0000],false),UvRect::from_signed([40.0000,6.0000],[6.0000,9.0000],false),UvRect::from_signed([52.0000,6.0000],[6.0000,9.0000],false)] },
UvReference { source: "panda", bone: "leg1", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([46.0000,0.0000],[6.0000,6.0000],false),UvRect::from_signed([52.0000,0.0000],[6.0000,6.0000],false),UvRect::from_signed([46.0000,6.0000],[6.0000,9.0000],false),UvRect::from_signed([58.0000,6.0000],[6.0000,9.0000],false),UvRect::from_signed([40.0000,6.0000],[6.0000,9.0000],false),UvRect::from_signed([52.0000,6.0000],[6.0000,9.0000],false)] },
UvReference { source: "panda", bone: "leg2", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([46.0000,0.0000],[6.0000,6.0000],false),UvRect::from_signed([52.0000,0.0000],[6.0000,6.0000],false),UvRect::from_signed([46.0000,6.0000],[6.0000,9.0000],false),UvRect::from_signed([58.0000,6.0000],[6.0000,9.0000],false),UvRect::from_signed([40.0000,6.0000],[6.0000,9.0000],false),UvRect::from_signed([52.0000,6.0000],[6.0000,9.0000],false)] },
UvReference { source: "panda", bone: "leg3", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([46.0000,0.0000],[6.0000,6.0000],false),UvRect::from_signed([52.0000,0.0000],[6.0000,6.0000],false),UvRect::from_signed([46.0000,6.0000],[6.0000,9.0000],false),UvRect::from_signed([58.0000,6.0000],[6.0000,9.0000],false),UvRect::from_signed([40.0000,6.0000],[6.0000,9.0000],false),UvRect::from_signed([52.0000,6.0000],[6.0000,9.0000],false)] },
UvReference { source: "parched", bone: "body", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([20.0000,16.0000],[8.0000,4.0000],false),UvRect::from_signed([28.0000,16.0000],[8.0000,4.0000],false),UvRect::from_signed([20.0000,20.0000],[8.0000,12.0000],false),UvRect::from_signed([32.0000,20.0000],[8.0000,12.0000],false),UvRect::from_signed([16.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([28.0000,20.0000],[4.0000,12.0000],false)] },
UvReference { source: "parched", bone: "body", cube: 1, nominal: [64,64], faces: [UvRect::from_signed([32.0000,0.0000],[8.0000,4.0000],false),UvRect::from_signed([40.0000,0.0000],[8.0000,4.0000],false),UvRect::from_signed([32.0000,4.0000],[8.0000,1.0000],false),UvRect::from_signed([44.0000,4.0000],[8.0000,1.0000],false),UvRect::from_signed([28.0000,4.0000],[4.0000,1.0000],false),UvRect::from_signed([40.0000,4.0000],[4.0000,1.0000],false)] },
UvReference { source: "parched", bone: "body", cube: 2, nominal: [64,64], faces: [UvRect::from_signed([20.0000,48.0000],[8.0000,4.0000],false),UvRect::from_signed([28.0000,48.0000],[8.0000,4.0000],false),UvRect::from_signed([20.0000,52.0000],[8.0000,12.0000],false),UvRect::from_signed([32.0000,52.0000],[8.0000,12.0000],false),UvRect::from_signed([16.0000,52.0000],[4.0000,12.0000],false),UvRect::from_signed([28.0000,52.0000],[4.0000,12.0000],false)] },
UvReference { source: "parched", bone: "head", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([8.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([16.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([8.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([24.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([0.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([16.0000,8.0000],[8.0000,8.0000],false)] },
UvReference { source: "parched", bone: "head", cube: 1, nominal: [64,64], faces: [UvRect::from_signed([8.0000,32.0000],[8.0000,8.0000],false),UvRect::from_signed([16.0000,32.0000],[8.0000,8.0000],false),UvRect::from_signed([8.0000,40.0000],[8.0000,8.0000],false),UvRect::from_signed([24.0000,40.0000],[8.0000,8.0000],false),UvRect::from_signed([0.0000,40.0000],[8.0000,8.0000],false),UvRect::from_signed([16.0000,40.0000],[8.0000,8.0000],false)] },
UvReference { source: "parched", bone: "rightArm", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([42.0000,16.0000],[2.0000,2.0000],false),UvRect::from_signed([44.0000,16.0000],[2.0000,2.0000],false),UvRect::from_signed([42.0000,18.0000],[2.0000,12.0000],false),UvRect::from_signed([46.0000,18.0000],[2.0000,12.0000],false),UvRect::from_signed([40.0000,18.0000],[2.0000,12.0000],false),UvRect::from_signed([44.0000,18.0000],[2.0000,12.0000],false)] },
UvReference { source: "parched", bone: "rightArm", cube: 1, nominal: [64,64], faces: [UvRect::from_signed([45.0000,33.0000],[3.0000,3.0000],false),UvRect::from_signed([48.0000,33.0000],[3.0000,3.0000],false),UvRect::from_signed([45.0000,36.0000],[3.0000,12.0000],false),UvRect::from_signed([51.0000,36.0000],[3.0000,12.0000],false),UvRect::from_signed([42.0000,36.0000],[3.0000,12.0000],false),UvRect::from_signed([48.0000,36.0000],[3.0000,12.0000],false)] },
UvReference { source: "parched", bone: "leftArm", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([58.0000,16.0000],[2.0000,2.0000],true),UvRect::from_signed([60.0000,16.0000],[2.0000,2.0000],true),UvRect::from_signed([58.0000,18.0000],[2.0000,12.0000],true),UvRect::from_signed([62.0000,18.0000],[2.0000,12.0000],true),UvRect::from_signed([56.0000,18.0000],[2.0000,12.0000],true),UvRect::from_signed([60.0000,18.0000],[2.0000,12.0000],true)] },
UvReference { source: "parched", bone: "leftArm", cube: 1, nominal: [64,64], faces: [UvRect::from_signed([43.0000,48.0000],[3.0000,3.0000],true),UvRect::from_signed([46.0000,48.0000],[3.0000,3.0000],true),UvRect::from_signed([43.0000,51.0000],[3.0000,12.0000],true),UvRect::from_signed([49.0000,51.0000],[3.0000,12.0000],true),UvRect::from_signed([40.0000,51.0000],[3.0000,12.0000],true),UvRect::from_signed([46.0000,51.0000],[3.0000,12.0000],true)] },
UvReference { source: "parched", bone: "rightLeg", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([2.0000,16.0000],[2.0000,2.0000],false),UvRect::from_signed([4.0000,16.0000],[2.0000,2.0000],false),UvRect::from_signed([2.0000,18.0000],[2.0000,12.0000],false),UvRect::from_signed([6.0000,18.0000],[2.0000,12.0000],false),UvRect::from_signed([0.0000,18.0000],[2.0000,12.0000],false),UvRect::from_signed([4.0000,18.0000],[2.0000,12.0000],false)] },
UvReference { source: "parched", bone: "rightLeg", cube: 1, nominal: [64,64], faces: [UvRect::from_signed([3.0000,49.0000],[3.0000,3.0000],false),UvRect::from_signed([6.0000,49.0000],[3.0000,3.0000],false),UvRect::from_signed([3.0000,52.0000],[3.0000,12.0000],false),UvRect::from_signed([9.0000,52.0000],[3.0000,12.0000],false),UvRect::from_signed([0.0000,52.0000],[3.0000,12.0000],false),UvRect::from_signed([6.0000,52.0000],[3.0000,12.0000],false)] },
UvReference { source: "parched", bone: "leftLeg", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([2.0000,16.0000],[2.0000,2.0000],true),UvRect::from_signed([4.0000,16.0000],[2.0000,2.0000],true),UvRect::from_signed([2.0000,18.0000],[2.0000,12.0000],true),UvRect::from_signed([6.0000,18.0000],[2.0000,12.0000],true),UvRect::from_signed([0.0000,18.0000],[2.0000,12.0000],true),UvRect::from_signed([4.0000,18.0000],[2.0000,12.0000],true)] },
UvReference { source: "parched", bone: "leftLeg", cube: 1, nominal: [64,64], faces: [UvRect::from_signed([7.0000,49.0000],[3.0000,3.0000],true),UvRect::from_signed([10.0000,49.0000],[3.0000,3.0000],true),UvRect::from_signed([7.0000,52.0000],[3.0000,12.0000],true),UvRect::from_signed([13.0000,52.0000],[3.0000,12.0000],true),UvRect::from_signed([4.0000,52.0000],[3.0000,12.0000],true),UvRect::from_signed([10.0000,52.0000],[3.0000,12.0000],true)] },
UvReference { source: "parrot", bone: "head", cube: 0, nominal: [32,32], faces: [UvRect::from_signed([4.0000,2.0000],[2.0000,2.0000],false),UvRect::from_signed([6.0000,2.0000],[2.0000,2.0000],false),UvRect::from_signed([4.0000,4.0000],[2.0000,3.0000],false),UvRect::from_signed([8.0000,4.0000],[2.0000,3.0000],false),UvRect::from_signed([2.0000,4.0000],[2.0000,3.0000],false),UvRect::from_signed([6.0000,4.0000],[2.0000,3.0000],false)] },
UvReference { source: "parrot", bone: "head2", cube: 0, nominal: [32,32], faces: [UvRect::from_signed([14.0000,0.0000],[2.0000,4.0000],false),UvRect::from_signed([16.0000,0.0000],[2.0000,4.0000],false),UvRect::from_signed([14.0000,4.0000],[2.0000,1.0000],false),UvRect::from_signed([20.0000,4.0000],[2.0000,1.0000],false),UvRect::from_signed([10.0000,4.0000],[4.0000,1.0000],false),UvRect::from_signed([16.0000,4.0000],[4.0000,1.0000],false)] },
UvReference { source: "parrot", bone: "beak1", cube: 0, nominal: [32,32], faces: [UvRect::from_signed([12.0000,7.0000],[1.0000,1.0000],false),UvRect::from_signed([13.0000,7.0000],[1.0000,1.0000],false),UvRect::from_signed([12.0000,8.0000],[1.0000,2.0000],false),UvRect::from_signed([14.0000,8.0000],[1.0000,2.0000],false),UvRect::from_signed([11.0000,8.0000],[1.0000,2.0000],false),UvRect::from_signed([13.0000,8.0000],[1.0000,2.0000],false)] },
UvReference { source: "parrot", bone: "beak2", cube: 0, nominal: [32,32], faces: [UvRect::from_signed([17.0000,7.0000],[1.0000,1.0000],false),UvRect::from_signed([18.0000,7.0000],[1.0000,1.0000],false),UvRect::from_signed([17.0000,8.0000],[1.0000,1.7000],false),UvRect::from_signed([19.0000,8.0000],[1.0000,1.7000],false),UvRect::from_signed([16.0000,8.0000],[1.0000,1.7000],false),UvRect::from_signed([18.0000,8.0000],[1.0000,1.7000],false)] },
UvReference { source: "parrot", bone: "body", cube: 0, nominal: [32,32], faces: [UvRect::from_signed([5.0000,8.0000],[3.0000,3.0000],false),UvRect::from_signed([8.0000,8.0000],[3.0000,3.0000],false),UvRect::from_signed([5.0000,11.0000],[3.0000,6.0000],false),UvRect::from_signed([11.0000,11.0000],[3.0000,6.0000],false),UvRect::from_signed([2.0000,11.0000],[3.0000,6.0000],false),UvRect::from_signed([8.0000,11.0000],[3.0000,6.0000],false)] },
UvReference { source: "parrot", bone: "tail", cube: 0, nominal: [32,32], faces: [UvRect::from_signed([23.0000,1.0000],[3.0000,1.0000],false),UvRect::from_signed([26.0000,1.0000],[3.0000,1.0000],false),UvRect::from_signed([23.0000,2.0000],[3.0000,4.0000],false),UvRect::from_signed([27.0000,2.0000],[3.0000,4.0000],false),UvRect::from_signed([22.0000,2.0000],[1.0000,4.0000],false),UvRect::from_signed([26.0000,2.0000],[1.0000,4.0000],false)] },
UvReference { source: "parrot", bone: "wing0", cube: 0, nominal: [32,32], faces: [UvRect::from_signed([22.0000,8.0000],[1.0000,3.0000],false),UvRect::from_signed([23.0000,8.0000],[1.0000,3.0000],false),UvRect::from_signed([22.0000,11.0000],[1.0000,5.0000],false),UvRect::from_signed([26.0000,11.0000],[1.0000,5.0000],false),UvRect::from_signed([19.0000,11.0000],[3.0000,5.0000],false),UvRect::from_signed([23.0000,11.0000],[3.0000,5.0000],false)] },
UvReference { source: "parrot", bone: "wing1", cube: 0, nominal: [32,32], faces: [UvRect::from_signed([22.0000,8.0000],[1.0000,3.0000],false),UvRect::from_signed([23.0000,8.0000],[1.0000,3.0000],false),UvRect::from_signed([22.0000,11.0000],[1.0000,5.0000],false),UvRect::from_signed([26.0000,11.0000],[1.0000,5.0000],false),UvRect::from_signed([19.0000,11.0000],[3.0000,5.0000],false),UvRect::from_signed([23.0000,11.0000],[3.0000,5.0000],false)] },
UvReference { source: "parrot", bone: "feather", cube: 0, nominal: [32,32], faces: [UvRect::from_signed([6.0000,18.0000],[0.0000,4.0000],false),UvRect::from_signed([6.0000,18.0000],[0.0000,4.0000],false),UvRect::from_signed([6.0000,22.0000],[0.0000,5.0000],false),UvRect::from_signed([10.0000,22.0000],[0.0000,5.0000],false),UvRect::from_signed([2.0000,22.0000],[4.0000,5.0000],false),UvRect::from_signed([6.0000,22.0000],[4.0000,5.0000],false)] },
UvReference { source: "parrot", bone: "leg0", cube: 0, nominal: [32,32], faces: [UvRect::from_signed([15.0000,18.0000],[1.0000,1.0000],false),UvRect::from_signed([16.0000,18.0000],[1.0000,1.0000],false),UvRect::from_signed([15.0000,19.0000],[1.0000,2.0000],false),UvRect::from_signed([17.0000,19.0000],[1.0000,2.0000],false),UvRect::from_signed([14.0000,19.0000],[1.0000,2.0000],false),UvRect::from_signed([16.0000,19.0000],[1.0000,2.0000],false)] },
UvReference { source: "parrot", bone: "leg1", cube: 0, nominal: [32,32], faces: [UvRect::from_signed([15.0000,18.0000],[1.0000,1.0000],false),UvRect::from_signed([16.0000,18.0000],[1.0000,1.0000],false),UvRect::from_signed([15.0000,19.0000],[1.0000,2.0000],false),UvRect::from_signed([17.0000,19.0000],[1.0000,2.0000],false),UvRect::from_signed([14.0000,19.0000],[1.0000,2.0000],false),UvRect::from_signed([16.0000,19.0000],[1.0000,2.0000],false)] },
UvReference { source: "phantom", bone: "body", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([9.0000,8.0000],[5.0000,9.0000],false),UvRect::from_signed([14.0000,8.0000],[5.0000,9.0000],false),UvRect::from_signed([9.0000,17.0000],[5.0000,3.0000],false),UvRect::from_signed([23.0000,17.0000],[5.0000,3.0000],false),UvRect::from_signed([0.0000,17.0000],[9.0000,3.0000],false),UvRect::from_signed([14.0000,17.0000],[9.0000,3.0000],false)] },
UvReference { source: "phantom", bone: "wing0", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([32.0000,12.0000],[6.0000,9.0000],false),UvRect::from_signed([38.0000,12.0000],[6.0000,9.0000],false),UvRect::from_signed([32.0000,21.0000],[6.0000,2.0000],false),UvRect::from_signed([47.0000,21.0000],[6.0000,2.0000],false),UvRect::from_signed([23.0000,21.0000],[9.0000,2.0000],false),UvRect::from_signed([38.0000,21.0000],[9.0000,2.0000],false)] },
UvReference { source: "phantom", bone: "wingtip0", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([25.0000,24.0000],[13.0000,9.0000],false),UvRect::from_signed([38.0000,24.0000],[13.0000,9.0000],false),UvRect::from_signed([25.0000,33.0000],[13.0000,1.0000],false),UvRect::from_signed([47.0000,33.0000],[13.0000,1.0000],false),UvRect::from_signed([16.0000,33.0000],[9.0000,1.0000],false),UvRect::from_signed([38.0000,33.0000],[9.0000,1.0000],false)] },
UvReference { source: "phantom", bone: "wing1", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([32.0000,12.0000],[6.0000,9.0000],true),UvRect::from_signed([38.0000,12.0000],[6.0000,9.0000],true),UvRect::from_signed([32.0000,21.0000],[6.0000,2.0000],true),UvRect::from_signed([47.0000,21.0000],[6.0000,2.0000],true),UvRect::from_signed([23.0000,21.0000],[9.0000,2.0000],true),UvRect::from_signed([38.0000,21.0000],[9.0000,2.0000],true)] },
UvReference { source: "phantom", bone: "wingtip1", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([25.0000,24.0000],[13.0000,9.0000],true),UvRect::from_signed([38.0000,24.0000],[13.0000,9.0000],true),UvRect::from_signed([25.0000,33.0000],[13.0000,1.0000],true),UvRect::from_signed([47.0000,33.0000],[13.0000,1.0000],true),UvRect::from_signed([16.0000,33.0000],[9.0000,1.0000],true),UvRect::from_signed([38.0000,33.0000],[9.0000,1.0000],true)] },
UvReference { source: "phantom", bone: "head", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([5.0000,0.0000],[7.0000,5.0000],false),UvRect::from_signed([12.0000,0.0000],[7.0000,5.0000],false),UvRect::from_signed([5.0000,5.0000],[7.0000,3.0000],false),UvRect::from_signed([17.0000,5.0000],[7.0000,3.0000],false),UvRect::from_signed([0.0000,5.0000],[5.0000,3.0000],false),UvRect::from_signed([12.0000,5.0000],[5.0000,3.0000],false)] },
UvReference { source: "phantom", bone: "tail", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([9.0000,20.0000],[3.0000,6.0000],false),UvRect::from_signed([12.0000,20.0000],[3.0000,6.0000],false),UvRect::from_signed([9.0000,26.0000],[3.0000,2.0000],false),UvRect::from_signed([18.0000,26.0000],[3.0000,2.0000],false),UvRect::from_signed([3.0000,26.0000],[6.0000,2.0000],false),UvRect::from_signed([12.0000,26.0000],[6.0000,2.0000],false)] },
UvReference { source: "phantom", bone: "tailtip", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([10.0000,29.0000],[1.0000,6.0000],false),UvRect::from_signed([11.0000,29.0000],[1.0000,6.0000],false),UvRect::from_signed([10.0000,35.0000],[1.0000,1.0000],false),UvRect::from_signed([17.0000,35.0000],[1.0000,1.0000],false),UvRect::from_signed([4.0000,35.0000],[6.0000,1.0000],false),UvRect::from_signed([11.0000,35.0000],[6.0000,1.0000],false)] },
UvReference { source: "pig", bone: "body", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([36.0000,8.0000],[10.0000,8.0000],false),UvRect::from_signed([46.0000,8.0000],[10.0000,8.0000],false),UvRect::from_signed([36.0000,16.0000],[10.0000,16.0000],false),UvRect::from_signed([54.0000,16.0000],[10.0000,16.0000],false),UvRect::from_signed([28.0000,16.0000],[8.0000,16.0000],false),UvRect::from_signed([46.0000,16.0000],[8.0000,16.0000],false)] },
UvReference { source: "pig", bone: "head", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([8.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([16.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([8.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([24.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([0.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([16.0000,8.0000],[8.0000,8.0000],false)] },
UvReference { source: "pig", bone: "head", cube: 1, nominal: [64,32], faces: [UvRect::from_signed([17.0000,16.0000],[4.0000,1.0000],false),UvRect::from_signed([21.0000,16.0000],[4.0000,1.0000],false),UvRect::from_signed([17.0000,17.0000],[4.0000,3.0000],false),UvRect::from_signed([22.0000,17.0000],[4.0000,3.0000],false),UvRect::from_signed([16.0000,17.0000],[1.0000,3.0000],false),UvRect::from_signed([21.0000,17.0000],[1.0000,3.0000],false)] },
UvReference { source: "pig", bone: "leg0", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([4.0000,20.0000],[4.0000,6.0000],false),UvRect::from_signed([12.0000,20.0000],[4.0000,6.0000],false),UvRect::from_signed([0.0000,20.0000],[4.0000,6.0000],false),UvRect::from_signed([8.0000,20.0000],[4.0000,6.0000],false)] },
UvReference { source: "pig", bone: "leg1", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],true),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],true),UvRect::from_signed([4.0000,20.0000],[4.0000,6.0000],true),UvRect::from_signed([12.0000,20.0000],[4.0000,6.0000],true),UvRect::from_signed([0.0000,20.0000],[4.0000,6.0000],true),UvRect::from_signed([8.0000,20.0000],[4.0000,6.0000],true)] },
UvReference { source: "pig", bone: "leg2", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([4.0000,20.0000],[4.0000,6.0000],false),UvRect::from_signed([12.0000,20.0000],[4.0000,6.0000],false),UvRect::from_signed([0.0000,20.0000],[4.0000,6.0000],false),UvRect::from_signed([8.0000,20.0000],[4.0000,6.0000],false)] },
UvReference { source: "pig", bone: "leg3", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],true),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],true),UvRect::from_signed([4.0000,20.0000],[4.0000,6.0000],true),UvRect::from_signed([12.0000,20.0000],[4.0000,6.0000],true),UvRect::from_signed([0.0000,20.0000],[4.0000,6.0000],true),UvRect::from_signed([8.0000,20.0000],[4.0000,6.0000],true)] },
UvReference { source: "pig.v3", bone: "head", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([8.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([16.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([8.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([24.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([0.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([16.0000,8.0000],[8.0000,8.0000],false)] },
UvReference { source: "pig.v3", bone: "head", cube: 1, nominal: [64,64], faces: [UvRect::from_signed([17.0000,16.0000],[4.0000,1.0000],false),UvRect::from_signed([21.0000,16.0000],[4.0000,1.0000],false),UvRect::from_signed([17.0000,17.0000],[4.0000,3.0000],false),UvRect::from_signed([22.0000,17.0000],[4.0000,3.0000],false),UvRect::from_signed([16.0000,17.0000],[1.0000,3.0000],false),UvRect::from_signed([21.0000,17.0000],[1.0000,3.0000],false)] },
UvReference { source: "pig.v3", bone: "body", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([36.0000,32.0000],[10.0000,8.0000],false),UvRect::from_signed([46.0000,32.0000],[10.0000,8.0000],false),UvRect::from_signed([36.0000,40.0000],[10.0000,16.0000],false),UvRect::from_signed([54.0000,40.0000],[10.0000,16.0000],false),UvRect::from_signed([28.0000,40.0000],[8.0000,16.0000],false),UvRect::from_signed([46.0000,40.0000],[8.0000,16.0000],false)] },
UvReference { source: "pig.v3", bone: "body", cube: 1, nominal: [64,64], faces: [UvRect::from_signed([36.0000,8.0000],[10.0000,8.0000],false),UvRect::from_signed([46.0000,8.0000],[10.0000,8.0000],false),UvRect::from_signed([36.0000,16.0000],[10.0000,16.0000],false),UvRect::from_signed([54.0000,16.0000],[10.0000,16.0000],false),UvRect::from_signed([28.0000,16.0000],[8.0000,16.0000],false),UvRect::from_signed([46.0000,16.0000],[8.0000,16.0000],false)] },
UvReference { source: "pig.v3", bone: "leg0", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([4.0000,20.0000],[4.0000,6.0000],false),UvRect::from_signed([12.0000,20.0000],[4.0000,6.0000],false),UvRect::from_signed([0.0000,20.0000],[4.0000,6.0000],false),UvRect::from_signed([8.0000,20.0000],[4.0000,6.0000],false)] },
UvReference { source: "pig.v3", bone: "leg1", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],true),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],true),UvRect::from_signed([4.0000,20.0000],[4.0000,6.0000],true),UvRect::from_signed([12.0000,20.0000],[4.0000,6.0000],true),UvRect::from_signed([0.0000,20.0000],[4.0000,6.0000],true),UvRect::from_signed([8.0000,20.0000],[4.0000,6.0000],true)] },
UvReference { source: "pig.v3", bone: "leg3", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],true),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],true),UvRect::from_signed([4.0000,20.0000],[4.0000,6.0000],true),UvRect::from_signed([12.0000,20.0000],[4.0000,6.0000],true),UvRect::from_signed([0.0000,20.0000],[4.0000,6.0000],true),UvRect::from_signed([8.0000,20.0000],[4.0000,6.0000],true)] },
UvReference { source: "pig.v3", bone: "leg2", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([4.0000,20.0000],[4.0000,6.0000],false),UvRect::from_signed([12.0000,20.0000],[4.0000,6.0000],false),UvRect::from_signed([0.0000,20.0000],[4.0000,6.0000],false),UvRect::from_signed([8.0000,20.0000],[4.0000,6.0000],false)] },
UvReference { source: "pig_v1.0", bone: "body", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([36.0000,8.0000],[10.0000,8.0000],false),UvRect::from_signed([46.0000,8.0000],[10.0000,8.0000],false),UvRect::from_signed([36.0000,16.0000],[10.0000,16.0000],false),UvRect::from_signed([54.0000,16.0000],[10.0000,16.0000],false),UvRect::from_signed([28.0000,16.0000],[8.0000,16.0000],false),UvRect::from_signed([46.0000,16.0000],[8.0000,16.0000],false)] },
UvReference { source: "pig_v1.0", bone: "head", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([8.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([16.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([8.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([24.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([0.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([16.0000,8.0000],[8.0000,8.0000],false)] },
UvReference { source: "pig_v1.0", bone: "head", cube: 1, nominal: [64,32], faces: [UvRect::from_signed([17.0000,16.0000],[4.0000,1.0000],false),UvRect::from_signed([21.0000,16.0000],[4.0000,1.0000],false),UvRect::from_signed([17.0000,17.0000],[4.0000,3.0000],false),UvRect::from_signed([22.0000,17.0000],[4.0000,3.0000],false),UvRect::from_signed([16.0000,17.0000],[1.0000,3.0000],false),UvRect::from_signed([21.0000,17.0000],[1.0000,3.0000],false)] },
UvReference { source: "pig_v1.0", bone: "leg0", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([4.0000,20.0000],[4.0000,6.0000],false),UvRect::from_signed([12.0000,20.0000],[4.0000,6.0000],false),UvRect::from_signed([0.0000,20.0000],[4.0000,6.0000],false),UvRect::from_signed([8.0000,20.0000],[4.0000,6.0000],false)] },
UvReference { source: "pig_v1.0", bone: "leg1", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],true),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],true),UvRect::from_signed([4.0000,20.0000],[4.0000,6.0000],true),UvRect::from_signed([12.0000,20.0000],[4.0000,6.0000],true),UvRect::from_signed([0.0000,20.0000],[4.0000,6.0000],true),UvRect::from_signed([8.0000,20.0000],[4.0000,6.0000],true)] },
UvReference { source: "pig_v1.0", bone: "leg2", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([4.0000,20.0000],[4.0000,6.0000],false),UvRect::from_signed([12.0000,20.0000],[4.0000,6.0000],false),UvRect::from_signed([0.0000,20.0000],[4.0000,6.0000],false),UvRect::from_signed([8.0000,20.0000],[4.0000,6.0000],false)] },
UvReference { source: "pig_v1.0", bone: "leg3", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],true),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],true),UvRect::from_signed([4.0000,20.0000],[4.0000,6.0000],true),UvRect::from_signed([12.0000,20.0000],[4.0000,6.0000],true),UvRect::from_signed([0.0000,20.0000],[4.0000,6.0000],true),UvRect::from_signed([8.0000,20.0000],[4.0000,6.0000],true)] },
UvReference { source: "pillager", bone: "head", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([8.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([16.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([8.0000,8.0000],[8.0000,10.0000],false),UvRect::from_signed([24.0000,8.0000],[8.0000,10.0000],false),UvRect::from_signed([0.0000,8.0000],[8.0000,10.0000],false),UvRect::from_signed([16.0000,8.0000],[8.0000,10.0000],false)] },
UvReference { source: "pillager", bone: "nose", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([26.0000,0.0000],[2.0000,2.0000],false),UvRect::from_signed([28.0000,0.0000],[2.0000,2.0000],false),UvRect::from_signed([26.0000,2.0000],[2.0000,4.0000],false),UvRect::from_signed([30.0000,2.0000],[2.0000,4.0000],false),UvRect::from_signed([24.0000,2.0000],[2.0000,4.0000],false),UvRect::from_signed([28.0000,2.0000],[2.0000,4.0000],false)] },
UvReference { source: "pillager", bone: "body", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([22.0000,20.0000],[8.0000,6.0000],false),UvRect::from_signed([30.0000,20.0000],[8.0000,6.0000],false),UvRect::from_signed([22.0000,26.0000],[8.0000,12.0000],false),UvRect::from_signed([36.0000,26.0000],[8.0000,12.0000],false),UvRect::from_signed([16.0000,26.0000],[6.0000,12.0000],false),UvRect::from_signed([30.0000,26.0000],[6.0000,12.0000],false)] },
UvReference { source: "pillager", bone: "body", cube: 1, nominal: [64,64], faces: [UvRect::from_signed([6.0000,38.0000],[8.0000,6.0000],false),UvRect::from_signed([14.0000,38.0000],[8.0000,6.0000],false),UvRect::from_signed([6.0000,44.0000],[8.0000,18.0000],false),UvRect::from_signed([20.0000,44.0000],[8.0000,18.0000],false),UvRect::from_signed([0.0000,44.0000],[6.0000,18.0000],false),UvRect::from_signed([14.0000,44.0000],[6.0000,18.0000],false)] },
UvReference { source: "pillager", bone: "leftLeg", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([4.0000,22.0000],[4.0000,4.0000],false),UvRect::from_signed([8.0000,22.0000],[4.0000,4.0000],false),UvRect::from_signed([4.0000,26.0000],[4.0000,12.0000],false),UvRect::from_signed([12.0000,26.0000],[4.0000,12.0000],false),UvRect::from_signed([0.0000,26.0000],[4.0000,12.0000],false),UvRect::from_signed([8.0000,26.0000],[4.0000,12.0000],false)] },
UvReference { source: "pillager", bone: "rightLeg", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([4.0000,22.0000],[4.0000,4.0000],true),UvRect::from_signed([8.0000,22.0000],[4.0000,4.0000],true),UvRect::from_signed([4.0000,26.0000],[4.0000,12.0000],true),UvRect::from_signed([12.0000,26.0000],[4.0000,12.0000],true),UvRect::from_signed([0.0000,26.0000],[4.0000,12.0000],true),UvRect::from_signed([8.0000,26.0000],[4.0000,12.0000],true)] },
UvReference { source: "pillager", bone: "rightarm", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([44.0000,46.0000],[4.0000,4.0000],false),UvRect::from_signed([48.0000,46.0000],[4.0000,4.0000],false),UvRect::from_signed([44.0000,50.0000],[4.0000,12.0000],false),UvRect::from_signed([52.0000,50.0000],[4.0000,12.0000],false),UvRect::from_signed([40.0000,50.0000],[4.0000,12.0000],false),UvRect::from_signed([48.0000,50.0000],[4.0000,12.0000],false)] },
UvReference { source: "pillager", bone: "leftarm", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([44.0000,46.0000],[4.0000,4.0000],true),UvRect::from_signed([48.0000,46.0000],[4.0000,4.0000],true),UvRect::from_signed([44.0000,50.0000],[4.0000,12.0000],true),UvRect::from_signed([52.0000,50.0000],[4.0000,12.0000],true),UvRect::from_signed([40.0000,50.0000],[4.0000,12.0000],true),UvRect::from_signed([48.0000,50.0000],[4.0000,12.0000],true)] },
UvReference { source: "polar_bear", bone: "head", cube: 0, nominal: [128,64], faces: [UvRect::from_signed([7.0000,0.0000],[7.0000,7.0000],false),UvRect::from_signed([14.0000,0.0000],[7.0000,7.0000],false),UvRect::from_signed([7.0000,7.0000],[7.0000,7.0000],false),UvRect::from_signed([21.0000,7.0000],[7.0000,7.0000],false),UvRect::from_signed([0.0000,7.0000],[7.0000,7.0000],false),UvRect::from_signed([14.0000,7.0000],[7.0000,7.0000],false)] },
UvReference { source: "polar_bear", bone: "head", cube: 1, nominal: [128,64], faces: [UvRect::from_signed([3.0000,44.0000],[5.0000,3.0000],false),UvRect::from_signed([8.0000,44.0000],[5.0000,3.0000],false),UvRect::from_signed([3.0000,47.0000],[5.0000,3.0000],false),UvRect::from_signed([11.0000,47.0000],[5.0000,3.0000],false),UvRect::from_signed([0.0000,47.0000],[3.0000,3.0000],false),UvRect::from_signed([8.0000,47.0000],[3.0000,3.0000],false)] },
UvReference { source: "polar_bear", bone: "head", cube: 2, nominal: [128,64], faces: [UvRect::from_signed([27.0000,0.0000],[2.0000,1.0000],false),UvRect::from_signed([29.0000,0.0000],[2.0000,1.0000],false),UvRect::from_signed([27.0000,1.0000],[2.0000,2.0000],false),UvRect::from_signed([30.0000,1.0000],[2.0000,2.0000],false),UvRect::from_signed([26.0000,1.0000],[1.0000,2.0000],false),UvRect::from_signed([29.0000,1.0000],[1.0000,2.0000],false)] },
UvReference { source: "polar_bear", bone: "head", cube: 3, nominal: [128,64], faces: [UvRect::from_signed([27.0000,0.0000],[2.0000,1.0000],true),UvRect::from_signed([29.0000,0.0000],[2.0000,1.0000],true),UvRect::from_signed([27.0000,1.0000],[2.0000,2.0000],true),UvRect::from_signed([30.0000,1.0000],[2.0000,2.0000],true),UvRect::from_signed([26.0000,1.0000],[1.0000,2.0000],true),UvRect::from_signed([29.0000,1.0000],[1.0000,2.0000],true)] },
UvReference { source: "polar_bear", bone: "body", cube: 0, nominal: [128,64], faces: [UvRect::from_signed([11.0000,19.0000],[14.0000,11.0000],false),UvRect::from_signed([25.0000,19.0000],[14.0000,11.0000],false),UvRect::from_signed([11.0000,30.0000],[14.0000,14.0000],false),UvRect::from_signed([36.0000,30.0000],[14.0000,14.0000],false),UvRect::from_signed([0.0000,30.0000],[11.0000,14.0000],false),UvRect::from_signed([25.0000,30.0000],[11.0000,14.0000],false)] },
UvReference { source: "polar_bear", bone: "body", cube: 1, nominal: [128,64], faces: [UvRect::from_signed([49.0000,0.0000],[12.0000,10.0000],false),UvRect::from_signed([61.0000,0.0000],[12.0000,10.0000],false),UvRect::from_signed([49.0000,10.0000],[12.0000,12.0000],false),UvRect::from_signed([71.0000,10.0000],[12.0000,12.0000],false),UvRect::from_signed([39.0000,10.0000],[10.0000,12.0000],false),UvRect::from_signed([61.0000,10.0000],[10.0000,12.0000],false)] },
UvReference { source: "polar_bear", bone: "leg0", cube: 0, nominal: [128,64], faces: [UvRect::from_signed([58.0000,22.0000],[4.0000,8.0000],false),UvRect::from_signed([62.0000,22.0000],[4.0000,8.0000],false),UvRect::from_signed([58.0000,30.0000],[4.0000,10.0000],false),UvRect::from_signed([70.0000,30.0000],[4.0000,10.0000],false),UvRect::from_signed([50.0000,30.0000],[8.0000,10.0000],false),UvRect::from_signed([62.0000,30.0000],[8.0000,10.0000],false)] },
UvReference { source: "polar_bear", bone: "leg1", cube: 0, nominal: [128,64], faces: [UvRect::from_signed([58.0000,22.0000],[4.0000,8.0000],false),UvRect::from_signed([62.0000,22.0000],[4.0000,8.0000],false),UvRect::from_signed([58.0000,30.0000],[4.0000,10.0000],false),UvRect::from_signed([70.0000,30.0000],[4.0000,10.0000],false),UvRect::from_signed([50.0000,30.0000],[8.0000,10.0000],false),UvRect::from_signed([62.0000,30.0000],[8.0000,10.0000],false)] },
UvReference { source: "polar_bear", bone: "leg2", cube: 0, nominal: [128,64], faces: [UvRect::from_signed([56.0000,40.0000],[4.0000,6.0000],false),UvRect::from_signed([60.0000,40.0000],[4.0000,6.0000],false),UvRect::from_signed([56.0000,46.0000],[4.0000,10.0000],false),UvRect::from_signed([66.0000,46.0000],[4.0000,10.0000],false),UvRect::from_signed([50.0000,46.0000],[6.0000,10.0000],false),UvRect::from_signed([60.0000,46.0000],[6.0000,10.0000],false)] },
UvReference { source: "polar_bear", bone: "leg3", cube: 0, nominal: [128,64], faces: [UvRect::from_signed([56.0000,40.0000],[4.0000,6.0000],false),UvRect::from_signed([60.0000,40.0000],[4.0000,6.0000],false),UvRect::from_signed([56.0000,46.0000],[4.0000,10.0000],false),UvRect::from_signed([66.0000,46.0000],[4.0000,10.0000],false),UvRect::from_signed([50.0000,46.0000],[6.0000,10.0000],false),UvRect::from_signed([60.0000,46.0000],[6.0000,10.0000],false)] },
UvReference { source: "rabbit", bone: "body", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([10.0000,0.0000],[6.0000,10.0000],true),UvRect::from_signed([16.0000,0.0000],[6.0000,10.0000],true),UvRect::from_signed([10.0000,10.0000],[6.0000,5.0000],true),UvRect::from_signed([26.0000,10.0000],[6.0000,5.0000],true),UvRect::from_signed([0.0000,10.0000],[10.0000,5.0000],true),UvRect::from_signed([16.0000,10.0000],[10.0000,5.0000],true)] },
UvReference { source: "rabbit", bone: "rearFootLeft", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([15.0000,24.0000],[2.0000,7.0000],true),UvRect::from_signed([17.0000,24.0000],[2.0000,7.0000],true),UvRect::from_signed([15.0000,31.0000],[2.0000,1.0000],true),UvRect::from_signed([24.0000,31.0000],[2.0000,1.0000],true),UvRect::from_signed([8.0000,31.0000],[7.0000,1.0000],true),UvRect::from_signed([17.0000,31.0000],[7.0000,1.0000],true)] },
UvReference { source: "rabbit", bone: "rearFootRight", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([33.0000,24.0000],[2.0000,7.0000],true),UvRect::from_signed([35.0000,24.0000],[2.0000,7.0000],true),UvRect::from_signed([33.0000,31.0000],[2.0000,1.0000],true),UvRect::from_signed([42.0000,31.0000],[2.0000,1.0000],true),UvRect::from_signed([26.0000,31.0000],[7.0000,1.0000],true),UvRect::from_signed([35.0000,31.0000],[7.0000,1.0000],true)] },
UvReference { source: "rabbit", bone: "haunchLeft", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([21.0000,15.0000],[2.0000,5.0000],true),UvRect::from_signed([23.0000,15.0000],[2.0000,5.0000],true),UvRect::from_signed([21.0000,20.0000],[2.0000,4.0000],true),UvRect::from_signed([28.0000,20.0000],[2.0000,4.0000],true),UvRect::from_signed([16.0000,20.0000],[5.0000,4.0000],true),UvRect::from_signed([23.0000,20.0000],[5.0000,4.0000],true)] },
UvReference { source: "rabbit", bone: "haunchRight", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([35.0000,15.0000],[2.0000,5.0000],true),UvRect::from_signed([37.0000,15.0000],[2.0000,5.0000],true),UvRect::from_signed([35.0000,20.0000],[2.0000,4.0000],true),UvRect::from_signed([42.0000,20.0000],[2.0000,4.0000],true),UvRect::from_signed([30.0000,20.0000],[5.0000,4.0000],true),UvRect::from_signed([37.0000,20.0000],[5.0000,4.0000],true)] },
UvReference { source: "rabbit", bone: "frontLegLeft", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([10.0000,15.0000],[2.0000,2.0000],true),UvRect::from_signed([12.0000,15.0000],[2.0000,2.0000],true),UvRect::from_signed([10.0000,17.0000],[2.0000,7.0000],true),UvRect::from_signed([14.0000,17.0000],[2.0000,7.0000],true),UvRect::from_signed([8.0000,17.0000],[2.0000,7.0000],true),UvRect::from_signed([12.0000,17.0000],[2.0000,7.0000],true)] },
UvReference { source: "rabbit", bone: "frontLegRight", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([2.0000,15.0000],[2.0000,2.0000],true),UvRect::from_signed([4.0000,15.0000],[2.0000,2.0000],true),UvRect::from_signed([2.0000,17.0000],[2.0000,7.0000],true),UvRect::from_signed([6.0000,17.0000],[2.0000,7.0000],true),UvRect::from_signed([0.0000,17.0000],[2.0000,7.0000],true),UvRect::from_signed([4.0000,17.0000],[2.0000,7.0000],true)] },
UvReference { source: "rabbit", bone: "head", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([37.0000,0.0000],[5.0000,5.0000],true),UvRect::from_signed([42.0000,0.0000],[5.0000,5.0000],true),UvRect::from_signed([37.0000,5.0000],[5.0000,4.0000],true),UvRect::from_signed([47.0000,5.0000],[5.0000,4.0000],true),UvRect::from_signed([32.0000,5.0000],[5.0000,4.0000],true),UvRect::from_signed([42.0000,5.0000],[5.0000,4.0000],true)] },
UvReference { source: "rabbit", bone: "earRight", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([59.0000,0.0000],[2.0000,1.0000],true),UvRect::from_signed([61.0000,0.0000],[2.0000,1.0000],true),UvRect::from_signed([59.0000,1.0000],[2.0000,5.0000],true),UvRect::from_signed([62.0000,1.0000],[2.0000,5.0000],true),UvRect::from_signed([58.0000,1.0000],[1.0000,5.0000],true),UvRect::from_signed([61.0000,1.0000],[1.0000,5.0000],true)] },
UvReference { source: "rabbit", bone: "earLeft", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([53.0000,0.0000],[2.0000,1.0000],true),UvRect::from_signed([55.0000,0.0000],[2.0000,1.0000],true),UvRect::from_signed([53.0000,1.0000],[2.0000,5.0000],true),UvRect::from_signed([56.0000,1.0000],[2.0000,5.0000],true),UvRect::from_signed([52.0000,1.0000],[1.0000,5.0000],true),UvRect::from_signed([55.0000,1.0000],[1.0000,5.0000],true)] },
UvReference { source: "rabbit", bone: "tail", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([54.0000,6.0000],[3.0000,2.0000],true),UvRect::from_signed([57.0000,6.0000],[3.0000,2.0000],true),UvRect::from_signed([54.0000,8.0000],[3.0000,3.0000],true),UvRect::from_signed([59.0000,8.0000],[3.0000,3.0000],true),UvRect::from_signed([52.0000,8.0000],[2.0000,3.0000],true),UvRect::from_signed([57.0000,8.0000],[2.0000,3.0000],true)] },
UvReference { source: "rabbit", bone: "nose", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([33.0000,9.0000],[1.0000,1.0000],true),UvRect::from_signed([34.0000,9.0000],[1.0000,1.0000],true),UvRect::from_signed([33.0000,10.0000],[1.0000,1.0000],true),UvRect::from_signed([35.0000,10.0000],[1.0000,1.0000],true),UvRect::from_signed([32.0000,10.0000],[1.0000,1.0000],true),UvRect::from_signed([34.0000,10.0000],[1.0000,1.0000],true)] },
UvReference { source: "ravager", bone: "body", cube: 0, nominal: [128,128], faces: [UvRect::from_signed([20.0000,55.0000],[14.0000,20.0000],false),UvRect::from_signed([34.0000,55.0000],[14.0000,20.0000],false),UvRect::from_signed([20.0000,75.0000],[14.0000,16.0000],false),UvRect::from_signed([54.0000,75.0000],[14.0000,16.0000],false),UvRect::from_signed([0.0000,75.0000],[20.0000,16.0000],false),UvRect::from_signed([34.0000,75.0000],[20.0000,16.0000],false)] },
UvReference { source: "ravager", bone: "body", cube: 1, nominal: [128,128], faces: [UvRect::from_signed([18.0000,91.0000],[12.0000,18.0000],false),UvRect::from_signed([30.0000,91.0000],[12.0000,18.0000],false),UvRect::from_signed([18.0000,109.0000],[12.0000,13.0000],false),UvRect::from_signed([48.0000,109.0000],[12.0000,13.0000],false),UvRect::from_signed([0.0000,109.0000],[18.0000,13.0000],false),UvRect::from_signed([30.0000,109.0000],[18.0000,13.0000],false)] },
UvReference { source: "ravager", bone: "mouth", cube: 0, nominal: [128,128], faces: [UvRect::from_signed([16.0000,36.0000],[16.0000,16.0000],false),UvRect::from_signed([32.0000,36.0000],[16.0000,16.0000],false),UvRect::from_signed([16.0000,52.0000],[16.0000,3.0000],false),UvRect::from_signed([48.0000,52.0000],[16.0000,3.0000],false),UvRect::from_signed([0.0000,52.0000],[16.0000,3.0000],false),UvRect::from_signed([32.0000,52.0000],[16.0000,3.0000],false)] },
UvReference { source: "ravager", bone: "neck", cube: 0, nominal: [128,128], faces: [UvRect::from_signed([86.0000,73.0000],[10.0000,18.0000],false),UvRect::from_signed([96.0000,73.0000],[10.0000,18.0000],false),UvRect::from_signed([86.0000,91.0000],[10.0000,10.0000],false),UvRect::from_signed([114.0000,91.0000],[10.0000,10.0000],false),UvRect::from_signed([68.0000,91.0000],[18.0000,10.0000],false),UvRect::from_signed([96.0000,91.0000],[18.0000,10.0000],false)] },
UvReference { source: "ravager", bone: "head", cube: 0, nominal: [128,128], faces: [UvRect::from_signed([16.0000,0.0000],[16.0000,16.0000],false),UvRect::from_signed([32.0000,0.0000],[16.0000,16.0000],false),UvRect::from_signed([16.0000,16.0000],[16.0000,20.0000],false),UvRect::from_signed([48.0000,16.0000],[16.0000,20.0000],false),UvRect::from_signed([0.0000,16.0000],[16.0000,20.0000],false),UvRect::from_signed([32.0000,16.0000],[16.0000,20.0000],false)] },
UvReference { source: "ravager", bone: "head", cube: 1, nominal: [128,128], faces: [UvRect::from_signed([4.0000,0.0000],[4.0000,4.0000],false),UvRect::from_signed([8.0000,0.0000],[4.0000,4.0000],false),UvRect::from_signed([4.0000,4.0000],[4.0000,8.0000],false),UvRect::from_signed([12.0000,4.0000],[4.0000,8.0000],false),UvRect::from_signed([0.0000,4.0000],[4.0000,8.0000],false),UvRect::from_signed([8.0000,4.0000],[4.0000,8.0000],false)] },
UvReference { source: "ravager", bone: "leg0", cube: 0, nominal: [128,128], faces: [UvRect::from_signed([104.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([112.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([104.0000,8.0000],[8.0000,37.0000],false),UvRect::from_signed([120.0000,8.0000],[8.0000,37.0000],false),UvRect::from_signed([96.0000,8.0000],[8.0000,37.0000],false),UvRect::from_signed([112.0000,8.0000],[8.0000,37.0000],false)] },
UvReference { source: "ravager", bone: "leg1", cube: 0, nominal: [128,128], faces: [UvRect::from_signed([104.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([112.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([104.0000,8.0000],[8.0000,37.0000],false),UvRect::from_signed([120.0000,8.0000],[8.0000,37.0000],false),UvRect::from_signed([96.0000,8.0000],[8.0000,37.0000],false),UvRect::from_signed([112.0000,8.0000],[8.0000,37.0000],false)] },
UvReference { source: "ravager", bone: "leg2", cube: 0, nominal: [128,128], faces: [UvRect::from_signed([72.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([80.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([72.0000,8.0000],[8.0000,37.0000],false),UvRect::from_signed([88.0000,8.0000],[8.0000,37.0000],false),UvRect::from_signed([64.0000,8.0000],[8.0000,37.0000],false),UvRect::from_signed([80.0000,8.0000],[8.0000,37.0000],false)] },
UvReference { source: "ravager", bone: "leg3", cube: 0, nominal: [128,128], faces: [UvRect::from_signed([72.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([80.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([72.0000,8.0000],[8.0000,37.0000],false),UvRect::from_signed([88.0000,8.0000],[8.0000,37.0000],false),UvRect::from_signed([64.0000,8.0000],[8.0000,37.0000],false),UvRect::from_signed([80.0000,8.0000],[8.0000,37.0000],false)] },
UvReference { source: "ravager", bone: "horns", cube: 0, nominal: [128,128], faces: [UvRect::from_signed([78.0000,55.0000],[2.0000,4.0000],false),UvRect::from_signed([80.0000,55.0000],[2.0000,4.0000],false),UvRect::from_signed([78.0000,59.0000],[2.0000,14.0000],false),UvRect::from_signed([84.0000,59.0000],[2.0000,14.0000],false),UvRect::from_signed([74.0000,59.0000],[4.0000,14.0000],false),UvRect::from_signed([80.0000,59.0000],[4.0000,14.0000],false)] },
UvReference { source: "ravager", bone: "horns", cube: 1, nominal: [128,128], faces: [UvRect::from_signed([78.0000,55.0000],[2.0000,4.0000],false),UvRect::from_signed([80.0000,55.0000],[2.0000,4.0000],false),UvRect::from_signed([78.0000,59.0000],[2.0000,14.0000],false),UvRect::from_signed([84.0000,59.0000],[2.0000,14.0000],false),UvRect::from_signed([74.0000,59.0000],[4.0000,14.0000],false),UvRect::from_signed([80.0000,59.0000],[4.0000,14.0000],false)] },
UvReference { source: "skeleton", bone: "body", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([20.0000,16.0000],[8.0000,4.0000],false),UvRect::from_signed([28.0000,16.0000],[8.0000,4.0000],false),UvRect::from_signed([20.0000,20.0000],[8.0000,12.0000],false),UvRect::from_signed([32.0000,20.0000],[8.0000,12.0000],false),UvRect::from_signed([16.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([28.0000,20.0000],[4.0000,12.0000],false)] },
UvReference { source: "skeleton", bone: "head", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([8.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([16.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([8.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([24.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([0.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([16.0000,8.0000],[8.0000,8.0000],false)] },
UvReference { source: "skeleton", bone: "hat", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([40.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([48.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([40.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([56.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([32.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([48.0000,8.0000],[8.0000,8.0000],false)] },
UvReference { source: "skeleton", bone: "rightArm", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([42.0000,16.0000],[2.0000,2.0000],false),UvRect::from_signed([44.0000,16.0000],[2.0000,2.0000],false),UvRect::from_signed([42.0000,18.0000],[2.0000,12.0000],false),UvRect::from_signed([46.0000,18.0000],[2.0000,12.0000],false),UvRect::from_signed([40.0000,18.0000],[2.0000,12.0000],false),UvRect::from_signed([44.0000,18.0000],[2.0000,12.0000],false)] },
UvReference { source: "skeleton", bone: "leftArm", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([42.0000,16.0000],[2.0000,2.0000],true),UvRect::from_signed([44.0000,16.0000],[2.0000,2.0000],true),UvRect::from_signed([42.0000,18.0000],[2.0000,12.0000],true),UvRect::from_signed([46.0000,18.0000],[2.0000,12.0000],true),UvRect::from_signed([40.0000,18.0000],[2.0000,12.0000],true),UvRect::from_signed([44.0000,18.0000],[2.0000,12.0000],true)] },
UvReference { source: "skeleton", bone: "rightLeg", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([2.0000,16.0000],[2.0000,2.0000],false),UvRect::from_signed([4.0000,16.0000],[2.0000,2.0000],false),UvRect::from_signed([2.0000,18.0000],[2.0000,12.0000],false),UvRect::from_signed([6.0000,18.0000],[2.0000,12.0000],false),UvRect::from_signed([0.0000,18.0000],[2.0000,12.0000],false),UvRect::from_signed([4.0000,18.0000],[2.0000,12.0000],false)] },
UvReference { source: "skeleton", bone: "leftLeg", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([2.0000,16.0000],[2.0000,2.0000],true),UvRect::from_signed([4.0000,16.0000],[2.0000,2.0000],true),UvRect::from_signed([2.0000,18.0000],[2.0000,12.0000],true),UvRect::from_signed([6.0000,18.0000],[2.0000,12.0000],true),UvRect::from_signed([0.0000,18.0000],[2.0000,12.0000],true),UvRect::from_signed([4.0000,18.0000],[2.0000,12.0000],true)] },
UvReference { source: "slime", bone: "cube", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([6.0000,16.0000],[6.0000,6.0000],false),UvRect::from_signed([12.0000,16.0000],[6.0000,6.0000],false),UvRect::from_signed([6.0000,22.0000],[6.0000,6.0000],false),UvRect::from_signed([18.0000,22.0000],[6.0000,6.0000],false),UvRect::from_signed([0.0000,22.0000],[6.0000,6.0000],false),UvRect::from_signed([12.0000,22.0000],[6.0000,6.0000],false)] },
UvReference { source: "slime", bone: "eye0", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([34.0000,0.0000],[2.0000,2.0000],false),UvRect::from_signed([36.0000,0.0000],[2.0000,2.0000],false),UvRect::from_signed([34.0000,2.0000],[2.0000,2.0000],false),UvRect::from_signed([38.0000,2.0000],[2.0000,2.0000],false),UvRect::from_signed([32.0000,2.0000],[2.0000,2.0000],false),UvRect::from_signed([36.0000,2.0000],[2.0000,2.0000],false)] },
UvReference { source: "slime", bone: "eye1", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([34.0000,4.0000],[2.0000,2.0000],false),UvRect::from_signed([36.0000,4.0000],[2.0000,2.0000],false),UvRect::from_signed([34.0000,6.0000],[2.0000,2.0000],false),UvRect::from_signed([38.0000,6.0000],[2.0000,2.0000],false),UvRect::from_signed([32.0000,6.0000],[2.0000,2.0000],false),UvRect::from_signed([36.0000,6.0000],[2.0000,2.0000],false)] },
UvReference { source: "slime", bone: "mouth", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([33.0000,8.0000],[1.0000,1.0000],false),UvRect::from_signed([34.0000,8.0000],[1.0000,1.0000],false),UvRect::from_signed([33.0000,9.0000],[1.0000,1.0000],false),UvRect::from_signed([35.0000,9.0000],[1.0000,1.0000],false),UvRect::from_signed([32.0000,9.0000],[1.0000,1.0000],false),UvRect::from_signed([34.0000,9.0000],[1.0000,1.0000],false)] },
UvReference { source: "spider", bone: "body0", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([6.0000,0.0000],[6.0000,6.0000],false),UvRect::from_signed([12.0000,0.0000],[6.0000,6.0000],false),UvRect::from_signed([6.0000,6.0000],[6.0000,6.0000],false),UvRect::from_signed([18.0000,6.0000],[6.0000,6.0000],false),UvRect::from_signed([0.0000,6.0000],[6.0000,6.0000],false),UvRect::from_signed([12.0000,6.0000],[6.0000,6.0000],false)] },
UvReference { source: "spider", bone: "head", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([40.0000,4.0000],[8.0000,8.0000],false),UvRect::from_signed([48.0000,4.0000],[8.0000,8.0000],false),UvRect::from_signed([40.0000,12.0000],[8.0000,8.0000],false),UvRect::from_signed([56.0000,12.0000],[8.0000,8.0000],false),UvRect::from_signed([32.0000,12.0000],[8.0000,8.0000],false),UvRect::from_signed([48.0000,12.0000],[8.0000,8.0000],false)] },
UvReference { source: "spider", bone: "body1", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([12.0000,12.0000],[10.0000,12.0000],false),UvRect::from_signed([22.0000,12.0000],[10.0000,12.0000],false),UvRect::from_signed([12.0000,24.0000],[10.0000,8.0000],false),UvRect::from_signed([34.0000,24.0000],[10.0000,8.0000],false),UvRect::from_signed([0.0000,24.0000],[12.0000,8.0000],false),UvRect::from_signed([22.0000,24.0000],[12.0000,8.0000],false)] },
UvReference { source: "spider", bone: "leg0", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([20.0000,0.0000],[16.0000,2.0000],false),UvRect::from_signed([36.0000,0.0000],[16.0000,2.0000],false),UvRect::from_signed([20.0000,2.0000],[16.0000,2.0000],false),UvRect::from_signed([38.0000,2.0000],[16.0000,2.0000],false),UvRect::from_signed([18.0000,2.0000],[2.0000,2.0000],false),UvRect::from_signed([36.0000,2.0000],[2.0000,2.0000],false)] },
UvReference { source: "spider", bone: "leg1", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([20.0000,0.0000],[16.0000,2.0000],true),UvRect::from_signed([36.0000,0.0000],[16.0000,2.0000],true),UvRect::from_signed([20.0000,2.0000],[16.0000,2.0000],true),UvRect::from_signed([38.0000,2.0000],[16.0000,2.0000],true),UvRect::from_signed([18.0000,2.0000],[2.0000,2.0000],true),UvRect::from_signed([36.0000,2.0000],[2.0000,2.0000],true)] },
UvReference { source: "spider", bone: "leg2", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([20.0000,0.0000],[16.0000,2.0000],false),UvRect::from_signed([36.0000,0.0000],[16.0000,2.0000],false),UvRect::from_signed([20.0000,2.0000],[16.0000,2.0000],false),UvRect::from_signed([38.0000,2.0000],[16.0000,2.0000],false),UvRect::from_signed([18.0000,2.0000],[2.0000,2.0000],false),UvRect::from_signed([36.0000,2.0000],[2.0000,2.0000],false)] },
UvReference { source: "spider", bone: "leg3", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([20.0000,0.0000],[16.0000,2.0000],true),UvRect::from_signed([36.0000,0.0000],[16.0000,2.0000],true),UvRect::from_signed([20.0000,2.0000],[16.0000,2.0000],true),UvRect::from_signed([38.0000,2.0000],[16.0000,2.0000],true),UvRect::from_signed([18.0000,2.0000],[2.0000,2.0000],true),UvRect::from_signed([36.0000,2.0000],[2.0000,2.0000],true)] },
UvReference { source: "spider", bone: "leg4", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([20.0000,0.0000],[16.0000,2.0000],false),UvRect::from_signed([36.0000,0.0000],[16.0000,2.0000],false),UvRect::from_signed([20.0000,2.0000],[16.0000,2.0000],false),UvRect::from_signed([38.0000,2.0000],[16.0000,2.0000],false),UvRect::from_signed([18.0000,2.0000],[2.0000,2.0000],false),UvRect::from_signed([36.0000,2.0000],[2.0000,2.0000],false)] },
UvReference { source: "spider", bone: "leg5", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([20.0000,0.0000],[16.0000,2.0000],true),UvRect::from_signed([36.0000,0.0000],[16.0000,2.0000],true),UvRect::from_signed([20.0000,2.0000],[16.0000,2.0000],true),UvRect::from_signed([38.0000,2.0000],[16.0000,2.0000],true),UvRect::from_signed([18.0000,2.0000],[2.0000,2.0000],true),UvRect::from_signed([36.0000,2.0000],[2.0000,2.0000],true)] },
UvReference { source: "spider", bone: "leg6", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([20.0000,0.0000],[16.0000,2.0000],false),UvRect::from_signed([36.0000,0.0000],[16.0000,2.0000],false),UvRect::from_signed([20.0000,2.0000],[16.0000,2.0000],false),UvRect::from_signed([38.0000,2.0000],[16.0000,2.0000],false),UvRect::from_signed([18.0000,2.0000],[2.0000,2.0000],false),UvRect::from_signed([36.0000,2.0000],[2.0000,2.0000],false)] },
UvReference { source: "spider", bone: "leg7", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([20.0000,0.0000],[16.0000,2.0000],true),UvRect::from_signed([36.0000,0.0000],[16.0000,2.0000],true),UvRect::from_signed([20.0000,2.0000],[16.0000,2.0000],true),UvRect::from_signed([38.0000,2.0000],[16.0000,2.0000],true),UvRect::from_signed([18.0000,2.0000],[2.0000,2.0000],true),UvRect::from_signed([36.0000,2.0000],[2.0000,2.0000],true)] },
UvReference { source: "stray", bone: "body", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([20.0000,16.0000],[8.0000,4.0000],false),UvRect::from_signed([28.0000,16.0000],[8.0000,4.0000],false),UvRect::from_signed([20.0000,20.0000],[8.0000,12.0000],false),UvRect::from_signed([32.0000,20.0000],[8.0000,12.0000],false),UvRect::from_signed([16.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([28.0000,20.0000],[4.0000,12.0000],false)] },
UvReference { source: "stray", bone: "head", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([8.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([16.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([8.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([24.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([0.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([16.0000,8.0000],[8.0000,8.0000],false)] },
UvReference { source: "stray", bone: "hat", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([40.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([48.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([40.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([56.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([32.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([48.0000,8.0000],[8.0000,8.0000],false)] },
UvReference { source: "stray", bone: "rightArm", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([42.0000,16.0000],[2.0000,2.0000],false),UvRect::from_signed([44.0000,16.0000],[2.0000,2.0000],false),UvRect::from_signed([42.0000,18.0000],[2.0000,12.0000],false),UvRect::from_signed([46.0000,18.0000],[2.0000,12.0000],false),UvRect::from_signed([40.0000,18.0000],[2.0000,12.0000],false),UvRect::from_signed([44.0000,18.0000],[2.0000,12.0000],false)] },
UvReference { source: "stray", bone: "leftArm", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([42.0000,16.0000],[2.0000,2.0000],true),UvRect::from_signed([44.0000,16.0000],[2.0000,2.0000],true),UvRect::from_signed([42.0000,18.0000],[2.0000,12.0000],true),UvRect::from_signed([46.0000,18.0000],[2.0000,12.0000],true),UvRect::from_signed([40.0000,18.0000],[2.0000,12.0000],true),UvRect::from_signed([44.0000,18.0000],[2.0000,12.0000],true)] },
UvReference { source: "stray", bone: "rightLeg", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([2.0000,16.0000],[2.0000,2.0000],false),UvRect::from_signed([4.0000,16.0000],[2.0000,2.0000],false),UvRect::from_signed([2.0000,18.0000],[2.0000,12.0000],false),UvRect::from_signed([6.0000,18.0000],[2.0000,12.0000],false),UvRect::from_signed([0.0000,18.0000],[2.0000,12.0000],false),UvRect::from_signed([4.0000,18.0000],[2.0000,12.0000],false)] },
UvReference { source: "stray", bone: "leftLeg", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([2.0000,16.0000],[2.0000,2.0000],true),UvRect::from_signed([4.0000,16.0000],[2.0000,2.0000],true),UvRect::from_signed([2.0000,18.0000],[2.0000,12.0000],true),UvRect::from_signed([6.0000,18.0000],[2.0000,12.0000],true),UvRect::from_signed([0.0000,18.0000],[2.0000,12.0000],true),UvRect::from_signed([4.0000,18.0000],[2.0000,12.0000],true)] },
UvReference { source: "turtle", bone: "head", cube: 0, nominal: [128,64], faces: [UvRect::from_signed([8.0000,0.0000],[6.0000,6.0000],false),UvRect::from_signed([14.0000,0.0000],[6.0000,6.0000],false),UvRect::from_signed([8.0000,6.0000],[6.0000,5.0000],false),UvRect::from_signed([20.0000,6.0000],[6.0000,5.0000],false),UvRect::from_signed([2.0000,6.0000],[6.0000,5.0000],false),UvRect::from_signed([14.0000,6.0000],[6.0000,5.0000],false)] },
UvReference { source: "turtle", bone: "eggbelly", cube: 0, nominal: [128,64], faces: [UvRect::from_signed([70.0000,33.0000],[9.0000,1.0000],false),UvRect::from_signed([79.0000,33.0000],[9.0000,1.0000],false),UvRect::from_signed([70.0000,34.0000],[9.0000,18.0000],false),UvRect::from_signed([80.0000,34.0000],[9.0000,18.0000],false),UvRect::from_signed([69.0000,34.0000],[1.0000,18.0000],false),UvRect::from_signed([79.0000,34.0000],[1.0000,18.0000],false)] },
UvReference { source: "turtle", bone: "body", cube: 0, nominal: [128,64], faces: [UvRect::from_signed([12.0000,37.0000],[19.0000,6.0000],false),UvRect::from_signed([31.0000,37.0000],[19.0000,6.0000],false),UvRect::from_signed([12.0000,43.0000],[19.0000,20.0000],false),UvRect::from_signed([37.0000,43.0000],[19.0000,20.0000],false),UvRect::from_signed([6.0000,43.0000],[6.0000,20.0000],false),UvRect::from_signed([31.0000,43.0000],[6.0000,20.0000],false)] },
UvReference { source: "turtle", bone: "body", cube: 1, nominal: [128,64], faces: [UvRect::from_signed([33.0000,1.0000],[11.0000,3.0000],false),UvRect::from_signed([44.0000,1.0000],[11.0000,3.0000],false),UvRect::from_signed([33.0000,4.0000],[11.0000,18.0000],false),UvRect::from_signed([47.0000,4.0000],[11.0000,18.0000],false),UvRect::from_signed([30.0000,4.0000],[3.0000,18.0000],false),UvRect::from_signed([44.0000,4.0000],[3.0000,18.0000],false)] },
UvReference { source: "turtle", bone: "leg0", cube: 0, nominal: [128,64], faces: [UvRect::from_signed([10.0000,23.0000],[4.0000,10.0000],false),UvRect::from_signed([14.0000,23.0000],[4.0000,10.0000],false),UvRect::from_signed([10.0000,33.0000],[4.0000,1.0000],false),UvRect::from_signed([24.0000,33.0000],[4.0000,1.0000],false),UvRect::from_signed([0.0000,33.0000],[10.0000,1.0000],false),UvRect::from_signed([14.0000,33.0000],[10.0000,1.0000],false)] },
UvReference { source: "turtle", bone: "leg1", cube: 0, nominal: [128,64], faces: [UvRect::from_signed([10.0000,12.0000],[4.0000,10.0000],false),UvRect::from_signed([14.0000,12.0000],[4.0000,10.0000],false),UvRect::from_signed([10.0000,22.0000],[4.0000,1.0000],false),UvRect::from_signed([24.0000,22.0000],[4.0000,1.0000],false),UvRect::from_signed([0.0000,22.0000],[10.0000,1.0000],false),UvRect::from_signed([14.0000,22.0000],[10.0000,1.0000],false)] },
UvReference { source: "turtle", bone: "leg2", cube: 0, nominal: [128,64], faces: [UvRect::from_signed([31.0000,30.0000],[13.0000,5.0000],false),UvRect::from_signed([44.0000,30.0000],[13.0000,5.0000],false),UvRect::from_signed([31.0000,35.0000],[13.0000,1.0000],false),UvRect::from_signed([49.0000,35.0000],[13.0000,1.0000],false),UvRect::from_signed([26.0000,35.0000],[5.0000,1.0000],false),UvRect::from_signed([44.0000,35.0000],[5.0000,1.0000],false)] },
UvReference { source: "turtle", bone: "leg3", cube: 0, nominal: [128,64], faces: [UvRect::from_signed([31.0000,24.0000],[13.0000,5.0000],false),UvRect::from_signed([44.0000,24.0000],[13.0000,5.0000],false),UvRect::from_signed([31.0000,29.0000],[13.0000,1.0000],false),UvRect::from_signed([49.0000,29.0000],[13.0000,1.0000],false),UvRect::from_signed([26.0000,29.0000],[5.0000,1.0000],false),UvRect::from_signed([44.0000,29.0000],[5.0000,1.0000],false)] },
UvReference { source: "vex", bone: "body", cube: 0, nominal: [32,32], faces: [UvRect::from_signed([2.0000,10.0000],[3.0000,2.0000],false),UvRect::from_signed([5.0000,10.0000],[3.0000,2.0000],false),UvRect::from_signed([2.0000,12.0000],[3.0000,4.0000],false),UvRect::from_signed([7.0000,12.0000],[3.0000,4.0000],false),UvRect::from_signed([0.0000,12.0000],[2.0000,4.0000],false),UvRect::from_signed([5.0000,12.0000],[2.0000,4.0000],false)] },
UvReference { source: "vex", bone: "body", cube: 1, nominal: [32,32], faces: [UvRect::from_signed([2.0000,16.0000],[3.0000,2.0000],false),UvRect::from_signed([5.0000,16.0000],[3.0000,2.0000],false),UvRect::from_signed([2.0000,18.0000],[3.0000,5.0000],false),UvRect::from_signed([7.0000,18.0000],[3.0000,5.0000],false),UvRect::from_signed([0.0000,18.0000],[2.0000,5.0000],false),UvRect::from_signed([5.0000,18.0000],[2.0000,5.0000],false)] },
UvReference { source: "vex", bone: "head", cube: 0, nominal: [32,32], faces: [UvRect::from_signed([5.0000,0.0000],[5.0000,5.0000],false),UvRect::from_signed([10.0000,0.0000],[5.0000,5.0000],false),UvRect::from_signed([5.0000,5.0000],[5.0000,5.0000],false),UvRect::from_signed([15.0000,5.0000],[5.0000,5.0000],false),UvRect::from_signed([0.0000,5.0000],[5.0000,5.0000],false),UvRect::from_signed([10.0000,5.0000],[5.0000,5.0000],false)] },
UvReference { source: "vex", bone: "rightArm", cube: 0, nominal: [32,32], faces: [UvRect::from_signed([25.0000,0.0000],[2.0000,2.0000],false),UvRect::from_signed([27.0000,0.0000],[2.0000,2.0000],false),UvRect::from_signed([25.0000,2.0000],[2.0000,4.0000],false),UvRect::from_signed([29.0000,2.0000],[2.0000,4.0000],false),UvRect::from_signed([23.0000,2.0000],[2.0000,4.0000],false),UvRect::from_signed([27.0000,2.0000],[2.0000,4.0000],false)] },
UvReference { source: "vex", bone: "leftArm", cube: 0, nominal: [32,32], faces: [UvRect::from_signed([25.0000,6.0000],[2.0000,2.0000],false),UvRect::from_signed([27.0000,6.0000],[2.0000,2.0000],false),UvRect::from_signed([25.0000,8.0000],[2.0000,4.0000],false),UvRect::from_signed([29.0000,8.0000],[2.0000,4.0000],false),UvRect::from_signed([23.0000,8.0000],[2.0000,4.0000],false),UvRect::from_signed([27.0000,8.0000],[2.0000,4.0000],false)] },
UvReference { source: "vex", bone: "leftWing", cube: 0, nominal: [32,32], faces: [UvRect::from_signed([16.0000,22.0000],[8.0000,0.0000],true),UvRect::from_signed([24.0000,22.0000],[8.0000,0.0000],true),UvRect::from_signed([16.0000,22.0000],[8.0000,5.0000],true),UvRect::from_signed([24.0000,22.0000],[8.0000,5.0000],true),UvRect::from_signed([16.0000,22.0000],[0.0000,5.0000],true),UvRect::from_signed([24.0000,22.0000],[0.0000,5.0000],true)] },
UvReference { source: "vex", bone: "rightWing", cube: 0, nominal: [32,32], faces: [UvRect::from_signed([16.0000,22.0000],[8.0000,0.0000],false),UvRect::from_signed([24.0000,22.0000],[8.0000,0.0000],false),UvRect::from_signed([16.0000,22.0000],[8.0000,5.0000],false),UvRect::from_signed([24.0000,22.0000],[8.0000,5.0000],false),UvRect::from_signed([16.0000,22.0000],[0.0000,5.0000],false),UvRect::from_signed([24.0000,22.0000],[0.0000,5.0000],false)] },
UvReference { source: "vindicator", bone: "head", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([8.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([16.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([8.0000,8.0000],[8.0000,10.0000],false),UvRect::from_signed([24.0000,8.0000],[8.0000,10.0000],false),UvRect::from_signed([0.0000,8.0000],[8.0000,10.0000],false),UvRect::from_signed([16.0000,8.0000],[8.0000,10.0000],false)] },
UvReference { source: "vindicator", bone: "nose", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([26.0000,0.0000],[2.0000,2.0000],false),UvRect::from_signed([28.0000,0.0000],[2.0000,2.0000],false),UvRect::from_signed([26.0000,2.0000],[2.0000,4.0000],false),UvRect::from_signed([30.0000,2.0000],[2.0000,4.0000],false),UvRect::from_signed([24.0000,2.0000],[2.0000,4.0000],false),UvRect::from_signed([28.0000,2.0000],[2.0000,4.0000],false)] },
UvReference { source: "vindicator", bone: "body", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([22.0000,20.0000],[8.0000,6.0000],false),UvRect::from_signed([30.0000,20.0000],[8.0000,6.0000],false),UvRect::from_signed([22.0000,26.0000],[8.0000,12.0000],false),UvRect::from_signed([36.0000,26.0000],[8.0000,12.0000],false),UvRect::from_signed([16.0000,26.0000],[6.0000,12.0000],false),UvRect::from_signed([30.0000,26.0000],[6.0000,12.0000],false)] },
UvReference { source: "vindicator", bone: "body", cube: 1, nominal: [64,64], faces: [UvRect::from_signed([6.0000,38.0000],[8.0000,6.0000],false),UvRect::from_signed([14.0000,38.0000],[8.0000,6.0000],false),UvRect::from_signed([6.0000,44.0000],[8.0000,18.0000],false),UvRect::from_signed([20.0000,44.0000],[8.0000,18.0000],false),UvRect::from_signed([0.0000,44.0000],[6.0000,18.0000],false),UvRect::from_signed([14.0000,44.0000],[6.0000,18.0000],false)] },
UvReference { source: "vindicator", bone: "arms", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([48.0000,22.0000],[4.0000,4.0000],false),UvRect::from_signed([52.0000,22.0000],[4.0000,4.0000],false),UvRect::from_signed([48.0000,26.0000],[4.0000,8.0000],false),UvRect::from_signed([56.0000,26.0000],[4.0000,8.0000],false),UvRect::from_signed([44.0000,26.0000],[4.0000,8.0000],false),UvRect::from_signed([52.0000,26.0000],[4.0000,8.0000],false)] },
UvReference { source: "vindicator", bone: "arms", cube: 1, nominal: [64,64], faces: [UvRect::from_signed([48.0000,22.0000],[4.0000,4.0000],false),UvRect::from_signed([52.0000,22.0000],[4.0000,4.0000],false),UvRect::from_signed([48.0000,26.0000],[4.0000,8.0000],false),UvRect::from_signed([56.0000,26.0000],[4.0000,8.0000],false),UvRect::from_signed([44.0000,26.0000],[4.0000,8.0000],false),UvRect::from_signed([52.0000,26.0000],[4.0000,8.0000],false)] },
UvReference { source: "vindicator", bone: "arms", cube: 2, nominal: [64,64], faces: [UvRect::from_signed([44.0000,38.0000],[8.0000,4.0000],false),UvRect::from_signed([52.0000,38.0000],[8.0000,4.0000],false),UvRect::from_signed([44.0000,42.0000],[8.0000,4.0000],false),UvRect::from_signed([56.0000,42.0000],[8.0000,4.0000],false),UvRect::from_signed([40.0000,42.0000],[4.0000,4.0000],false),UvRect::from_signed([52.0000,42.0000],[4.0000,4.0000],false)] },
UvReference { source: "vindicator", bone: "leg0", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([4.0000,22.0000],[4.0000,4.0000],false),UvRect::from_signed([8.0000,22.0000],[4.0000,4.0000],false),UvRect::from_signed([4.0000,26.0000],[4.0000,12.0000],false),UvRect::from_signed([12.0000,26.0000],[4.0000,12.0000],false),UvRect::from_signed([0.0000,26.0000],[4.0000,12.0000],false),UvRect::from_signed([8.0000,26.0000],[4.0000,12.0000],false)] },
UvReference { source: "vindicator", bone: "leg1", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([4.0000,22.0000],[4.0000,4.0000],true),UvRect::from_signed([8.0000,22.0000],[4.0000,4.0000],true),UvRect::from_signed([4.0000,26.0000],[4.0000,12.0000],true),UvRect::from_signed([12.0000,26.0000],[4.0000,12.0000],true),UvRect::from_signed([0.0000,26.0000],[4.0000,12.0000],true),UvRect::from_signed([8.0000,26.0000],[4.0000,12.0000],true)] },
UvReference { source: "vindicator", bone: "rightArm", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([44.0000,46.0000],[4.0000,4.0000],false),UvRect::from_signed([48.0000,46.0000],[4.0000,4.0000],false),UvRect::from_signed([44.0000,50.0000],[4.0000,12.0000],false),UvRect::from_signed([52.0000,50.0000],[4.0000,12.0000],false),UvRect::from_signed([40.0000,50.0000],[4.0000,12.0000],false),UvRect::from_signed([48.0000,50.0000],[4.0000,12.0000],false)] },
UvReference { source: "vindicator", bone: "leftArm", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([44.0000,46.0000],[4.0000,4.0000],true),UvRect::from_signed([48.0000,46.0000],[4.0000,4.0000],true),UvRect::from_signed([44.0000,50.0000],[4.0000,12.0000],true),UvRect::from_signed([52.0000,50.0000],[4.0000,12.0000],true),UvRect::from_signed([40.0000,50.0000],[4.0000,12.0000],true),UvRect::from_signed([48.0000,50.0000],[4.0000,12.0000],true)] },
UvReference { source: "witch", bone: "nose", cube: 0, nominal: [64,128], faces: [UvRect::from_signed([1.0000,0.0000],[1.0000,1.0000],false),UvRect::from_signed([2.0000,0.0000],[1.0000,1.0000],false),UvRect::from_signed([1.0000,1.0000],[1.0000,1.0000],false),UvRect::from_signed([3.0000,1.0000],[1.0000,1.0000],false),UvRect::from_signed([0.0000,1.0000],[1.0000,1.0000],false),UvRect::from_signed([2.0000,1.0000],[1.0000,1.0000],false)] },
UvReference { source: "witch", bone: "hat", cube: 0, nominal: [64,128], faces: [UvRect::from_signed([10.0000,64.0000],[10.0000,10.0000],false),UvRect::from_signed([20.0000,64.0000],[10.0000,10.0000],false),UvRect::from_signed([10.0000,74.0000],[10.0000,2.0000],false),UvRect::from_signed([30.0000,74.0000],[10.0000,2.0000],false),UvRect::from_signed([0.0000,74.0000],[10.0000,2.0000],false),UvRect::from_signed([20.0000,74.0000],[10.0000,2.0000],false)] },
UvReference { source: "witch", bone: "hat2", cube: 0, nominal: [64,128], faces: [UvRect::from_signed([7.0000,76.0000],[7.0000,7.0000],false),UvRect::from_signed([14.0000,76.0000],[7.0000,7.0000],false),UvRect::from_signed([7.0000,83.0000],[7.0000,4.0000],false),UvRect::from_signed([21.0000,83.0000],[7.0000,4.0000],false),UvRect::from_signed([0.0000,83.0000],[7.0000,4.0000],false),UvRect::from_signed([14.0000,83.0000],[7.0000,4.0000],false)] },
UvReference { source: "witch", bone: "hat3", cube: 0, nominal: [64,128], faces: [UvRect::from_signed([4.0000,87.0000],[4.0000,4.0000],false),UvRect::from_signed([8.0000,87.0000],[4.0000,4.0000],false),UvRect::from_signed([4.0000,91.0000],[4.0000,4.0000],false),UvRect::from_signed([12.0000,91.0000],[4.0000,4.0000],false),UvRect::from_signed([0.0000,91.0000],[4.0000,4.0000],false),UvRect::from_signed([8.0000,91.0000],[4.0000,4.0000],false)] },
UvReference { source: "witch", bone: "hat4", cube: 0, nominal: [64,128], faces: [UvRect::from_signed([1.0000,95.0000],[1.0000,1.0000],false),UvRect::from_signed([2.0000,95.0000],[1.0000,1.0000],false),UvRect::from_signed([1.0000,96.0000],[1.0000,2.0000],false),UvRect::from_signed([3.0000,96.0000],[1.0000,2.0000],false),UvRect::from_signed([0.0000,96.0000],[1.0000,2.0000],false),UvRect::from_signed([2.0000,96.0000],[1.0000,2.0000],false)] },
UvReference { source: "wolf", bone: "head", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([4.0000,0.0000],[6.0000,4.0000],false),UvRect::from_signed([10.0000,0.0000],[6.0000,4.0000],false),UvRect::from_signed([4.0000,4.0000],[6.0000,6.0000],false),UvRect::from_signed([14.0000,4.0000],[6.0000,6.0000],false),UvRect::from_signed([0.0000,4.0000],[4.0000,6.0000],false),UvRect::from_signed([10.0000,4.0000],[4.0000,6.0000],false)] },
UvReference { source: "wolf", bone: "head", cube: 1, nominal: [64,32], faces: [UvRect::from_signed([17.0000,14.0000],[2.0000,1.0000],false),UvRect::from_signed([19.0000,14.0000],[2.0000,1.0000],false),UvRect::from_signed([17.0000,15.0000],[2.0000,2.0000],false),UvRect::from_signed([20.0000,15.0000],[2.0000,2.0000],false),UvRect::from_signed([16.0000,15.0000],[1.0000,2.0000],false),UvRect::from_signed([19.0000,15.0000],[1.0000,2.0000],false)] },
UvReference { source: "wolf", bone: "head", cube: 2, nominal: [64,32], faces: [UvRect::from_signed([17.0000,14.0000],[2.0000,1.0000],false),UvRect::from_signed([19.0000,14.0000],[2.0000,1.0000],false),UvRect::from_signed([17.0000,15.0000],[2.0000,2.0000],false),UvRect::from_signed([20.0000,15.0000],[2.0000,2.0000],false),UvRect::from_signed([16.0000,15.0000],[1.0000,2.0000],false),UvRect::from_signed([19.0000,15.0000],[1.0000,2.0000],false)] },
UvReference { source: "wolf", bone: "head", cube: 3, nominal: [64,32], faces: [UvRect::from_signed([4.0000,10.0000],[3.0000,4.0000],false),UvRect::from_signed([7.0000,10.0000],[3.0000,4.0000],false),UvRect::from_signed([4.0000,14.0000],[3.0000,3.0000],false),UvRect::from_signed([11.0000,14.0000],[3.0000,3.0000],false),UvRect::from_signed([0.0000,14.0000],[4.0000,3.0000],false),UvRect::from_signed([7.0000,14.0000],[4.0000,3.0000],false)] },
UvReference { source: "wolf", bone: "body", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([24.0000,14.0000],[6.0000,6.0000],false),UvRect::from_signed([30.0000,14.0000],[6.0000,6.0000],false),UvRect::from_signed([24.0000,20.0000],[6.0000,9.0000],false),UvRect::from_signed([36.0000,20.0000],[6.0000,9.0000],false),UvRect::from_signed([18.0000,20.0000],[6.0000,9.0000],false),UvRect::from_signed([30.0000,20.0000],[6.0000,9.0000],false)] },
UvReference { source: "wolf", bone: "upperBody", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([28.0000,0.0000],[8.0000,7.0000],false),UvRect::from_signed([36.0000,0.0000],[8.0000,7.0000],false),UvRect::from_signed([28.0000,7.0000],[8.0000,6.0000],false),UvRect::from_signed([43.0000,7.0000],[8.0000,6.0000],false),UvRect::from_signed([21.0000,7.0000],[7.0000,6.0000],false),UvRect::from_signed([36.0000,7.0000],[7.0000,6.0000],false)] },
UvReference { source: "wolf", bone: "leg0", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([2.0000,18.0000],[2.0000,2.0000],false),UvRect::from_signed([4.0000,18.0000],[2.0000,2.0000],false),UvRect::from_signed([2.0000,20.0000],[2.0000,8.0000],false),UvRect::from_signed([6.0000,20.0000],[2.0000,8.0000],false),UvRect::from_signed([0.0000,20.0000],[2.0000,8.0000],false),UvRect::from_signed([4.0000,20.0000],[2.0000,8.0000],false)] },
UvReference { source: "wolf", bone: "leg1", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([2.0000,18.0000],[2.0000,2.0000],false),UvRect::from_signed([4.0000,18.0000],[2.0000,2.0000],false),UvRect::from_signed([2.0000,20.0000],[2.0000,8.0000],false),UvRect::from_signed([6.0000,20.0000],[2.0000,8.0000],false),UvRect::from_signed([0.0000,20.0000],[2.0000,8.0000],false),UvRect::from_signed([4.0000,20.0000],[2.0000,8.0000],false)] },
UvReference { source: "wolf", bone: "leg2", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([2.0000,18.0000],[2.0000,2.0000],false),UvRect::from_signed([4.0000,18.0000],[2.0000,2.0000],false),UvRect::from_signed([2.0000,20.0000],[2.0000,8.0000],false),UvRect::from_signed([6.0000,20.0000],[2.0000,8.0000],false),UvRect::from_signed([0.0000,20.0000],[2.0000,8.0000],false),UvRect::from_signed([4.0000,20.0000],[2.0000,8.0000],false)] },
UvReference { source: "wolf", bone: "leg3", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([2.0000,18.0000],[2.0000,2.0000],false),UvRect::from_signed([4.0000,18.0000],[2.0000,2.0000],false),UvRect::from_signed([2.0000,20.0000],[2.0000,8.0000],false),UvRect::from_signed([6.0000,20.0000],[2.0000,8.0000],false),UvRect::from_signed([0.0000,20.0000],[2.0000,8.0000],false),UvRect::from_signed([4.0000,20.0000],[2.0000,8.0000],false)] },
UvReference { source: "wolf", bone: "tail", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([11.0000,18.0000],[2.0000,2.0000],false),UvRect::from_signed([13.0000,18.0000],[2.0000,2.0000],false),UvRect::from_signed([11.0000,20.0000],[2.0000,8.0000],false),UvRect::from_signed([15.0000,20.0000],[2.0000,8.0000],false),UvRect::from_signed([9.0000,20.0000],[2.0000,8.0000],false),UvRect::from_signed([13.0000,20.0000],[2.0000,8.0000],false)] },
UvReference { source: "zombie", bone: "body", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([20.0000,16.0000],[8.0000,4.0000],false),UvRect::from_signed([28.0000,16.0000],[8.0000,4.0000],false),UvRect::from_signed([20.0000,20.0000],[8.0000,12.0000],false),UvRect::from_signed([32.0000,20.0000],[8.0000,12.0000],false),UvRect::from_signed([16.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([28.0000,20.0000],[4.0000,12.0000],false)] },
UvReference { source: "zombie", bone: "head", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([8.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([16.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([8.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([24.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([0.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([16.0000,8.0000],[8.0000,8.0000],false)] },
UvReference { source: "zombie", bone: "hat", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([40.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([48.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([40.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([56.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([32.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([48.0000,8.0000],[8.0000,8.0000],false)] },
UvReference { source: "zombie", bone: "rightArm", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([44.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([48.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([44.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([52.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([40.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([48.0000,20.0000],[4.0000,12.0000],false)] },
UvReference { source: "zombie", bone: "leftArm", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([44.0000,16.0000],[4.0000,4.0000],true),UvRect::from_signed([48.0000,16.0000],[4.0000,4.0000],true),UvRect::from_signed([44.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([52.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([40.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([48.0000,20.0000],[4.0000,12.0000],true)] },
UvReference { source: "zombie", bone: "rightLeg", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([4.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([12.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([0.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([8.0000,20.0000],[4.0000,12.0000],false)] },
UvReference { source: "zombie", bone: "leftLeg", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],true),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],true),UvRect::from_signed([4.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([12.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([0.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([8.0000,20.0000],[4.0000,12.0000],true)] },
UvReference { source: "zombie_villager_v2", bone: "head", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([8.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([16.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([8.0000,8.0000],[8.0000,10.0000],false),UvRect::from_signed([24.0000,8.0000],[8.0000,10.0000],false),UvRect::from_signed([0.0000,8.0000],[8.0000,10.0000],false),UvRect::from_signed([16.0000,8.0000],[8.0000,10.0000],false)] },
UvReference { source: "zombie_villager_v2", bone: "head", cube: 1, nominal: [64,64], faces: [UvRect::from_signed([26.0000,0.0000],[2.0000,2.0000],false),UvRect::from_signed([28.0000,0.0000],[2.0000,2.0000],false),UvRect::from_signed([26.0000,2.0000],[2.0000,4.0000],false),UvRect::from_signed([30.0000,2.0000],[2.0000,4.0000],false),UvRect::from_signed([24.0000,2.0000],[2.0000,4.0000],false),UvRect::from_signed([28.0000,2.0000],[2.0000,4.0000],false)] },
UvReference { source: "zombie_villager_v2", bone: "helmet", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([40.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([48.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([40.0000,8.0000],[8.0000,10.0000],false),UvRect::from_signed([56.0000,8.0000],[8.0000,10.0000],false),UvRect::from_signed([32.0000,8.0000],[8.0000,10.0000],false),UvRect::from_signed([48.0000,8.0000],[8.0000,10.0000],false)] },
UvReference { source: "zombie_villager_v2", bone: "brim", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([31.0000,47.0000],[16.0000,1.0000],false),UvRect::from_signed([47.0000,47.0000],[16.0000,1.0000],false),UvRect::from_signed([31.0000,48.0000],[16.0000,16.0000],false),UvRect::from_signed([48.0000,48.0000],[16.0000,16.0000],false),UvRect::from_signed([30.0000,48.0000],[1.0000,16.0000],false),UvRect::from_signed([47.0000,48.0000],[1.0000,16.0000],false)] },
UvReference { source: "zombie_villager_v2", bone: "body", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([22.0000,20.0000],[8.0000,6.0000],false),UvRect::from_signed([30.0000,20.0000],[8.0000,6.0000],false),UvRect::from_signed([22.0000,26.0000],[8.0000,12.0000],false),UvRect::from_signed([36.0000,26.0000],[8.0000,12.0000],false),UvRect::from_signed([16.0000,26.0000],[6.0000,12.0000],false),UvRect::from_signed([30.0000,26.0000],[6.0000,12.0000],false)] },
UvReference { source: "zombie_villager_v2", bone: "body", cube: 1, nominal: [64,64], faces: [UvRect::from_signed([6.0000,38.0000],[8.0000,6.0000],false),UvRect::from_signed([14.0000,38.0000],[8.0000,6.0000],false),UvRect::from_signed([6.0000,44.0000],[8.0000,18.0000],false),UvRect::from_signed([20.0000,44.0000],[8.0000,18.0000],false),UvRect::from_signed([0.0000,44.0000],[6.0000,18.0000],false),UvRect::from_signed([14.0000,44.0000],[6.0000,18.0000],false)] },
UvReference { source: "zombie_villager_v2", bone: "rightArm", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([48.0000,22.0000],[4.0000,4.0000],false),UvRect::from_signed([52.0000,22.0000],[4.0000,4.0000],false),UvRect::from_signed([48.0000,26.0000],[4.0000,12.0000],false),UvRect::from_signed([56.0000,26.0000],[4.0000,12.0000],false),UvRect::from_signed([44.0000,26.0000],[4.0000,12.0000],false),UvRect::from_signed([52.0000,26.0000],[4.0000,12.0000],false)] },
UvReference { source: "zombie_villager_v2", bone: "leftArm", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([48.0000,22.0000],[4.0000,4.0000],true),UvRect::from_signed([52.0000,22.0000],[4.0000,4.0000],true),UvRect::from_signed([48.0000,26.0000],[4.0000,12.0000],true),UvRect::from_signed([56.0000,26.0000],[4.0000,12.0000],true),UvRect::from_signed([44.0000,26.0000],[4.0000,12.0000],true),UvRect::from_signed([52.0000,26.0000],[4.0000,12.0000],true)] },
UvReference { source: "zombie_villager_v2", bone: "rightLeg", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([4.0000,22.0000],[4.0000,4.0000],false),UvRect::from_signed([8.0000,22.0000],[4.0000,4.0000],false),UvRect::from_signed([4.0000,26.0000],[4.0000,12.0000],false),UvRect::from_signed([12.0000,26.0000],[4.0000,12.0000],false),UvRect::from_signed([0.0000,26.0000],[4.0000,12.0000],false),UvRect::from_signed([8.0000,26.0000],[4.0000,12.0000],false)] },
UvReference { source: "zombie_villager_v2", bone: "leftLeg", cube: 0, nominal: [64,64], faces: [UvRect::from_signed([4.0000,22.0000],[4.0000,4.0000],true),UvRect::from_signed([8.0000,22.0000],[4.0000,4.0000],true),UvRect::from_signed([4.0000,26.0000],[4.0000,12.0000],true),UvRect::from_signed([12.0000,26.0000],[4.0000,12.0000],true),UvRect::from_signed([0.0000,26.0000],[4.0000,12.0000],true),UvRect::from_signed([8.0000,26.0000],[4.0000,12.0000],true)] },
UvReference { source: "zombie_pigman", bone: "body", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([20.0000,16.0000],[8.0000,4.0000],false),UvRect::from_signed([28.0000,16.0000],[8.0000,4.0000],false),UvRect::from_signed([20.0000,20.0000],[8.0000,12.0000],false),UvRect::from_signed([32.0000,20.0000],[8.0000,12.0000],false),UvRect::from_signed([16.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([28.0000,20.0000],[4.0000,12.0000],false)] },
UvReference { source: "zombie_pigman", bone: "head", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([8.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([16.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([8.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([24.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([0.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([16.0000,8.0000],[8.0000,8.0000],false)] },
UvReference { source: "zombie_pigman", bone: "hat", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([40.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([48.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([40.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([56.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([32.0000,8.0000],[8.0000,8.0000],false),UvRect::from_signed([48.0000,8.0000],[8.0000,8.0000],false)] },
UvReference { source: "zombie_pigman", bone: "rightArm", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([44.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([48.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([44.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([52.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([40.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([48.0000,20.0000],[4.0000,12.0000],false)] },
UvReference { source: "zombie_pigman", bone: "leftArm", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([44.0000,16.0000],[4.0000,4.0000],true),UvRect::from_signed([48.0000,16.0000],[4.0000,4.0000],true),UvRect::from_signed([44.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([52.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([40.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([48.0000,20.0000],[4.0000,12.0000],true)] },
UvReference { source: "zombie_pigman", bone: "rightLeg", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([4.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([12.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([0.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([8.0000,20.0000],[4.0000,12.0000],false)] },
UvReference { source: "zombie_pigman", bone: "leftLeg", cube: 0, nominal: [64,32], faces: [UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],true),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],true),UvRect::from_signed([4.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([12.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([0.0000,20.0000],[4.0000,12.0000],true),UvRect::from_signed([8.0000,20.0000],[4.0000,12.0000],true)] },
];
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cow_has_head_body_horns_and_four_separate_legs() {
        let m = model(Species::Cow, AtlasLayout::Legacy, ClimateSkin::Temperate);
        assert!(m.parts.iter().any(|p| p.name == "head"));
        assert!(m.parts.iter().any(|p| p.name == "body"));
        assert_eq!(
            m.parts
                .iter()
                .filter(|p| p.name.starts_with("leg_"))
                .count(),
            4
        );
        assert_eq!(
            m.parts
                .iter()
                .filter(|p| p.name.starts_with("horn_"))
                .count(),
            2
        );
    }
    #[test]
    fn all_biome_and_scoped_event_species_have_distinct_canonical_ids() {
        let biome = [
            "armadillo",
            "bogged",
            "camel",
            "chicken",
            "cow",
            "creeper",
            "donkey",
            "drowned",
            "enderman",
            "fox",
            "frog",
            "goat",
            "horse",
            "husk",
            "llama",
            "mooshroom",
            "ocelot",
            "panda",
            "parched",
            "parrot",
            "pig",
            "polar_bear",
            "rabbit",
            "sheep",
            "skeleton",
            "slime",
            "spider",
            "stray",
            "turtle",
            "witch",
            "wolf",
            "zombie",
            "zombie_horse",
            "zombie_villager",
        ];
        for id in biome {
            assert!(
                Species::from_id(&format!("minecraft:{id}")).is_some(),
                "{id}"
            );
        }
        assert_eq!(ALL_SPECIES.len(), 50);
        let ids: std::collections::HashSet<_> = ALL_SPECIES.iter().map(|s| s.id()).collect();
        assert_eq!(ids.len(), 50);
        assert!(Species::from_id("minecraft:armor_stand").is_none());
        assert!(Species::from_id("minecraft:cushion").is_none());
    }
    #[test]
    fn every_layout_and_climate_has_finite_grounded_multi_part_geometry() {
        for species in ALL_SPECIES {
            for layout in [
                AtlasLayout::Legacy,
                AtlasLayout::Modern,
                AtlasLayout::Bedrock,
            ] {
                for climate in [ClimateSkin::Temperate, ClimateSkin::Warm, ClimateSkin::Cold] {
                    let m = model(*species, layout, climate);
                    assert!(m.parts.len() >= 5, "{species:?}");
                    let (min, max) = m.bounds().unwrap();
                    assert!(min[2] >= 0.0, "{species:?}");
                    assert!(max[2] > min[2]);
                    for p in m.parts {
                        let zero = (0..3).filter(|i| p.max[*i] == p.min[*i]).count();
                        assert_eq!(zero, usize::from(p.shape == Shape::Plane));
                        for axis in 0..3 {
                            assert!(
                                p.min[axis].is_finite()
                                    && p.max[axis].is_finite()
                                    && p.max[axis] >= p.min[axis]
                            );
                        }
                        if let Some(uv) = p.uv {
                            for face in uv.faces {
                                assert!(
                                    face.normalized(uv.nominal).is_some(),
                                    "{species:?}:{}",
                                    p.name
                                );
                            }
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn literal_family_silhouettes_keep_diagnostic_anatomy() {
        let names = |s| {
            model(s, AtlasLayout::Modern, ClimateSkin::Temperate)
                .parts
                .into_iter()
                .map(|p| p.name)
                .collect::<Vec<_>>()
        };
        for (s, part) in [
            (Species::Camel, "hump"),
            (Species::Rabbit, "hind_haunch_left"),
            (Species::Turtle, "shell"),
            (Species::Frog, "eye_left"),
            (Species::Chicken, "wattle"),
            (Species::Parrot, "long_tail"),
            (Species::Witch, "hat_tip"),
            (Species::Mooshroom, "mushroom_front"),
            (Species::Creaking, "bark_crown"),
            (Species::Ravager, "broad_nose"),
            (Species::Pillager, "crossbow_bow"),
        ] {
            assert!(names(s).contains(&part), "{s:?}");
        }
        assert_eq!(
            names(Species::Spider)
                .iter()
                .filter(|n| n.starts_with("leg_"))
                .count(),
            8
        );
        assert!(!names(Species::Pig).contains(&"horn_left"));
        assert!(!names(Species::Horse).contains(&"hump"));
        let height = |s| {
            model(s, AtlasLayout::Modern, ClimateSkin::Temperate)
                .bounds()
                .unwrap()
                .1[2]
        };
        assert!(height(Species::Enderman) > height(Species::Zombie) + 0.8);
        assert!(height(Species::Donkey) > height(Species::Horse) + 0.1);
    }
    #[test]
    fn cow_and_pig_layout_changes_are_independent_of_texture_resolution() {
        for s in [Species::Cow, Species::Pig] {
            let old = model(s, AtlasLayout::Legacy, ClimateSkin::Temperate);
            let new = model(s, AtlasLayout::Modern, ClimateSkin::Temperate);
            let h = |m: &Model| {
                m.parts
                    .iter()
                    .find(|p| p.name == "head")
                    .unwrap()
                    .uv
                    .unwrap()
            };
            assert_eq!(h(&old).nominal, [64, 32]);
            assert_eq!(h(&new).nominal, [64, 64]);
            let rect = h(&new).faces[2];
            let px = rect.pixels([64, 64], [128, 128]).unwrap();
            assert_eq!(px.max[0], rect.max[0] * 2.0);
        }
        // Chicken source supplies a 64x32 layout for both climates: distinguish source
        // identity, without inventing a 64x64 chicken atlas simply because cows changed.
        assert_ne!(
            uv_source(Species::Chicken, AtlasLayout::Modern, ClimateSkin::Cold),
            uv_source(Species::Chicken, AtlasLayout::Legacy, ClimateSkin::Cold)
        );
    }
    #[test]
    fn atlas_normalization_rejects_invalid_dimensions_and_preserves_mirrors() {
        let r = UvRect::from_signed([8.0, 12.0], [-4.0, -8.0], false);
        assert!(r.flip_u && r.flip_v);
        assert_eq!(r.min, [4.0, 4.0]);
        assert!(r.normalized([0, 32]).is_none());
        assert!(r.pixels([16, 16], [0, 32]).is_none());
        assert!(r.normalized([4, 4]).is_none());
        assert_eq!(r.pixels([16, 16], [32, 64]).unwrap().max, [16.0, 48.0]);
    }
    #[test]
    fn every_reference_rectangle_is_bounded_without_silent_clipping() {
        for r in UV_REFERENCES {
            assert!(!r.source.is_empty());
            for uv in r.faces {
                assert!(
                    uv.normalized(r.nominal).is_some(),
                    "{} {}",
                    r.source,
                    r.bone
                );
            }
        }
    }
    #[test]
    fn part_pivot_rotation_is_included_in_bounds() {
        let mut m = model(Species::Bee, AtlasLayout::Modern, ClimateSkin::Temperate);
        let p = &mut m.parts[0];
        p.rotation[2] = std::f32::consts::FRAC_PI_2;
        let corners = p.corners();
        let min_x = corners.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min);
        let max_x = corners
            .iter()
            .map(|p| p[0])
            .fold(f32::NEG_INFINITY, f32::max);
        assert!((max_x - min_x - 0.7).abs() < 0.0001);
        let bounds = m.bounds().unwrap();
        for p in &m.parts {
            for c in p.corners() {
                for (i, coordinate) in c.iter().enumerate() {
                    assert!(*coordinate >= bounds.0[i] && *coordinate <= bounds.1[i]);
                }
            }
        }
    }
    #[test]
    fn jockey_compositions_keep_mounts_and_place_scaled_riders_above_them() {
        for event in [
            SurfaceEvent::SpiderJockey,
            SurfaceEvent::ChickenJockey,
            SurfaceEvent::HorseJockey,
        ] {
            let models = event_models(event, AtlasLayout::Modern);
            assert_eq!(models.len(), 2);
            assert!(models[1].bounds().unwrap().0[2] >= 0.6);
        }
        assert_eq!(
            event_models(SurfaceEvent::LightningPig, AtlasLayout::Legacy)[0].species,
            Species::ZombifiedPiglin
        );
        assert!(
            event_models(SurfaceEvent::ChargedCreeper, AtlasLayout::Modern)[0]
                .parts
                .iter()
                .any(|p| p.texture_semantic == "minecraft:creeper/charged")
        );
    }
    #[test]
    fn absent_sourced_anatomy_is_explicit_not_a_guessed_region() {
        let cow = model(Species::Cow, AtlasLayout::Legacy, ClimateSkin::Temperate);
        assert!(cow
            .parts
            .iter()
            .any(|p| p.uv_status() == UvStatus::UnsupportedPart));
        assert!(cow
            .parts
            .iter()
            .filter_map(|p| p.uv)
            .all(|u| u.status == UvStatus::BedrockReferenceJavaUnverified));
        let bedrock = model(Species::Cow, AtlasLayout::Bedrock, ClimateSkin::Temperate);
        assert!(bedrock
            .parts
            .iter()
            .filter_map(|p| p.uv)
            .all(|u| u.status == UvStatus::BedrockReference));
    }
    #[test]
    fn ears_touch_the_authored_head_instead_of_floating_at_generic_coordinates() {
        for species in [
            Species::Rabbit,
            Species::Horse,
            Species::Donkey,
            Species::Pig,
            Species::Camel,
            Species::Wolf,
            Species::Panda,
        ] {
            let m = model(species, AtlasLayout::Modern, ClimateSkin::Temperate);
            let head = m.parts.iter().find(|p| p.name == "head").unwrap();
            for ear in m.parts.iter().filter(|p| p.name.starts_with("ear_")) {
                assert!(
                    (0..3)
                        .all(|i| ear.min[i] <= head.max[i] + 0.001
                            && ear.max[i] >= head.min[i] - 0.001),
                    "{species:?}:{}",
                    ear.name
                );
            }
        }
    }
}

#[rustfmt::skip]
const AUTHORED_NOMINAL_REFERENCES: &[UvReference]=&[
UvReference {source:"villager_v2",bone:"head",cube:0,nominal:[64,64],faces:[UvRect::from_signed([8.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([16.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([8.0000,8.0000],[8.0000,10.0000],false),UvRect::from_signed([24.0000,8.0000],[8.0000,10.0000],false),UvRect::from_signed([0.0000,8.0000],[8.0000,10.0000],false),UvRect::from_signed([16.0000,8.0000],[8.0000,10.0000],false)]},
UvReference {source:"villager_v2",bone:"helmet",cube:0,nominal:[64,64],faces:[UvRect::from_signed([40.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([48.0000,0.0000],[8.0000,8.0000],false),UvRect::from_signed([40.0000,8.0000],[8.0000,10.0000],false),UvRect::from_signed([56.0000,8.0000],[8.0000,10.0000],false),UvRect::from_signed([32.0000,8.0000],[8.0000,10.0000],false),UvRect::from_signed([48.0000,8.0000],[8.0000,10.0000],false)]},
UvReference {source:"villager_v2",bone:"brim",cube:0,nominal:[64,64],faces:[UvRect::from_signed([31.0000,47.0000],[16.0000,1.0000],false),UvRect::from_signed([47.0000,47.0000],[16.0000,1.0000],false),UvRect::from_signed([31.0000,48.0000],[16.0000,16.0000],false),UvRect::from_signed([48.0000,48.0000],[16.0000,16.0000],false),UvRect::from_signed([30.0000,48.0000],[1.0000,16.0000],false),UvRect::from_signed([47.0000,48.0000],[1.0000,16.0000],false)]},
UvReference {source:"villager_v2",bone:"nose",cube:0,nominal:[64,64],faces:[UvRect::from_signed([26.0000,0.0000],[2.0000,2.0000],false),UvRect::from_signed([28.0000,0.0000],[2.0000,2.0000],false),UvRect::from_signed([26.0000,2.0000],[2.0000,4.0000],false),UvRect::from_signed([30.0000,2.0000],[2.0000,4.0000],false),UvRect::from_signed([24.0000,2.0000],[2.0000,4.0000],false),UvRect::from_signed([28.0000,2.0000],[2.0000,4.0000],false)]},
UvReference {source:"villager_v2",bone:"body",cube:0,nominal:[64,64],faces:[UvRect::from_signed([22.0000,20.0000],[8.0000,6.0000],false),UvRect::from_signed([30.0000,20.0000],[8.0000,6.0000],false),UvRect::from_signed([22.0000,26.0000],[8.0000,12.0000],false),UvRect::from_signed([36.0000,26.0000],[8.0000,12.0000],false),UvRect::from_signed([16.0000,26.0000],[6.0000,12.0000],false),UvRect::from_signed([30.0000,26.0000],[6.0000,12.0000],false)]},
UvReference {source:"villager_v2",bone:"body",cube:1,nominal:[64,64],faces:[UvRect::from_signed([6.0000,38.0000],[8.0000,6.0000],false),UvRect::from_signed([14.0000,38.0000],[8.0000,6.0000],false),UvRect::from_signed([6.0000,44.0000],[8.0000,18.0000],false),UvRect::from_signed([20.0000,44.0000],[8.0000,18.0000],false),UvRect::from_signed([0.0000,44.0000],[6.0000,18.0000],false),UvRect::from_signed([14.0000,44.0000],[6.0000,18.0000],false)]},
UvReference {source:"villager_v2",bone:"arms",cube:0,nominal:[64,64],faces:[UvRect::from_signed([44.0000,38.0000],[8.0000,4.0000],false),UvRect::from_signed([52.0000,38.0000],[8.0000,4.0000],false),UvRect::from_signed([44.0000,42.0000],[8.0000,4.0000],false),UvRect::from_signed([56.0000,42.0000],[8.0000,4.0000],false),UvRect::from_signed([40.0000,42.0000],[4.0000,4.0000],false),UvRect::from_signed([52.0000,42.0000],[4.0000,4.0000],false)]},
UvReference {source:"villager_v2",bone:"arms",cube:1,nominal:[64,64],faces:[UvRect::from_signed([48.0000,22.0000],[4.0000,4.0000],false),UvRect::from_signed([52.0000,22.0000],[4.0000,4.0000],false),UvRect::from_signed([48.0000,26.0000],[4.0000,8.0000],false),UvRect::from_signed([56.0000,26.0000],[4.0000,8.0000],false),UvRect::from_signed([44.0000,26.0000],[4.0000,8.0000],false),UvRect::from_signed([52.0000,26.0000],[4.0000,8.0000],false)]},
UvReference {source:"villager_v2",bone:"arms",cube:2,nominal:[64,64],faces:[UvRect::from_signed([48.0000,22.0000],[4.0000,4.0000],true),UvRect::from_signed([52.0000,22.0000],[4.0000,4.0000],true),UvRect::from_signed([48.0000,26.0000],[4.0000,8.0000],true),UvRect::from_signed([56.0000,26.0000],[4.0000,8.0000],true),UvRect::from_signed([44.0000,26.0000],[4.0000,8.0000],true),UvRect::from_signed([52.0000,26.0000],[4.0000,8.0000],true)]},
UvReference {source:"villager_v2",bone:"leg0",cube:0,nominal:[64,64],faces:[UvRect::from_signed([4.0000,22.0000],[4.0000,4.0000],false),UvRect::from_signed([8.0000,22.0000],[4.0000,4.0000],false),UvRect::from_signed([4.0000,26.0000],[4.0000,12.0000],false),UvRect::from_signed([12.0000,26.0000],[4.0000,12.0000],false),UvRect::from_signed([0.0000,26.0000],[4.0000,12.0000],false),UvRect::from_signed([8.0000,26.0000],[4.0000,12.0000],false)]},
UvReference {source:"villager_v2",bone:"leg1",cube:0,nominal:[64,64],faces:[UvRect::from_signed([4.0000,22.0000],[4.0000,4.0000],true),UvRect::from_signed([8.0000,22.0000],[4.0000,4.0000],true),UvRect::from_signed([4.0000,26.0000],[4.0000,12.0000],true),UvRect::from_signed([12.0000,26.0000],[4.0000,12.0000],true),UvRect::from_signed([0.0000,26.0000],[4.0000,12.0000],true),UvRect::from_signed([8.0000,26.0000],[4.0000,12.0000],true)]},
UvReference {source:"sheep",bone:"body",cube:0,nominal:[64,32],faces:[UvRect::from_signed([34.0000,8.0000],[8.0000,6.0000],false),UvRect::from_signed([42.0000,8.0000],[8.0000,6.0000],false),UvRect::from_signed([34.0000,14.0000],[8.0000,16.0000],false),UvRect::from_signed([48.0000,14.0000],[8.0000,16.0000],false),UvRect::from_signed([28.0000,14.0000],[6.0000,16.0000],false),UvRect::from_signed([42.0000,14.0000],[6.0000,16.0000],false)]},
UvReference {source:"sheep",bone:"head",cube:0,nominal:[64,32],faces:[UvRect::from_signed([8.0000,0.0000],[6.0000,8.0000],false),UvRect::from_signed([14.0000,0.0000],[6.0000,8.0000],false),UvRect::from_signed([8.0000,8.0000],[6.0000,6.0000],false),UvRect::from_signed([22.0000,8.0000],[6.0000,6.0000],false),UvRect::from_signed([0.0000,8.0000],[8.0000,6.0000],false),UvRect::from_signed([14.0000,8.0000],[8.0000,6.0000],false)]},
UvReference {source:"sheep",bone:"leg0",cube:0,nominal:[64,32],faces:[UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([4.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([12.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([0.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([8.0000,20.0000],[4.0000,12.0000],false)]},
UvReference {source:"sheep",bone:"leg1",cube:0,nominal:[64,32],faces:[UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([4.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([12.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([0.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([8.0000,20.0000],[4.0000,12.0000],false)]},
UvReference {source:"sheep",bone:"leg2",cube:0,nominal:[64,32],faces:[UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([4.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([12.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([0.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([8.0000,20.0000],[4.0000,12.0000],false)]},
UvReference {source:"sheep",bone:"leg3",cube:0,nominal:[64,32],faces:[UvRect::from_signed([4.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([8.0000,16.0000],[4.0000,4.0000],false),UvRect::from_signed([4.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([12.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([0.0000,20.0000],[4.0000,12.0000],false),UvRect::from_signed([8.0000,20.0000],[4.0000,12.0000],false)]},
];
/// Header/digest evidence supplied by the actual pack decoder, not guessed from its brand.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AtlasEvidence {
    pub semantic: String,
    pub resource_id: String,
    pub png_dimensions: [u32; 2],
    pub encoded_sha256: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompatibilityError {
    InvalidEvidence,
    ConflictingEvidence,
    MissingSourcedRegion,
    InvalidUv,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompatibilityOutcome {
    Matched,
    Unmatched,
}
/// A deliberately authored mapping policy; never claim these new regions are native.
/// One immutable atlas identity is allowed per semantic. Rebuild a fresh model when
/// changing the selected asset bank. Identical repeat evidence is idempotent;
/// unmatched semantics return `Unmatched` without recording evidence or changing parts.
pub fn apply_authored_compatibility(
    model: &mut Model,
    atlas: &AtlasEvidence,
) -> Result<CompatibilityOutcome, CompatibilityError> {
    if atlas.png_dimensions.contains(&0)
        || atlas.semantic.is_empty()
        || atlas.resource_id.is_empty()
        || atlas.encoded_sha256.len() != 64
        || !atlas.encoded_sha256.bytes().all(|c| c.is_ascii_hexdigit())
    {
        return Err(CompatibilityError::InvalidEvidence);
    }
    let active_evidence = model
        .atlas_evidence
        .iter()
        .find(|e| e.semantic == atlas.semantic);
    if active_evidence.is_some_and(|active| active != atlas) {
        return Err(CompatibilityError::ConflictingEvidence);
    }
    let has_active_evidence = active_evidence.is_some();
    let source = uv_source(model.species, model.layout, model.climate);
    // Prepare atomically: a rejected source must not leave partially reassigned anatomy.
    let mut parts = model.parts.clone();
    let mut matched = false;
    for part in parts
        .iter_mut()
        .filter(|p| p.texture_semantic == atlas.semantic)
    {
        matched = true;
        if part.uv.is_none() {
            let head_detail = [
                "head", "ear", "horn", "muzzle", "snout", "beak", "comb", "hat", "nose", "eye",
                "crest", "wattle", "whisker",
            ]
            .iter()
            .any(|n| part.name.contains(n));
            let role = if head_detail {
                "head"
            } else if part.name.contains("leg") || part.name.contains("foot") {
                "leg"
            } else if part.name.contains("wing") {
                "wing"
            } else if part.name.contains("tail") {
                "tail"
            } else {
                "body"
            };
            let region = if source == "villager_v2" || source == "sheep" {
                let bone = if source == "sheep" {
                    if head_detail {
                        "head"
                    } else if role == "leg" {
                        "leg0"
                    } else {
                        "body"
                    }
                } else if part.name.contains("nose") {
                    "nose"
                } else if head_detail {
                    "head"
                } else if part.name.contains("arm") {
                    "arms"
                } else if role == "leg" {
                    "leg0"
                } else {
                    "body"
                };
                AUTHORED_NOMINAL_REFERENCES
                    .iter()
                    .find(|r| r.source == source && r.bone == bone && r.cube == 0)
            } else {
                reference(source, role)
                    .or_else(|| reference(source, "body"))
                    .or_else(|| reference(source, "head"))
                    .or_else(|| {
                        // Published derived humanoids may supply only hats/noses. Reuse
                        // recorded villager regions as authored family compatibility.
                        if matches!(source, "witch" | "vindicator" | "evoker" | "pillager") {
                            let bone = if head_detail {
                                "head"
                            } else if role == "leg" {
                                "leg0"
                            } else {
                                "body"
                            };
                            AUTHORED_NOMINAL_REFERENCES.iter().find(|r| {
                                r.source == "villager_v2" && r.bone == bone && r.cube == 0
                            })
                        } else {
                            None
                        }
                    })
            }
            .ok_or(CompatibilityError::MissingSourcedRegion)?;
            part.uv = Some(PartUv {
                nominal: if region.source != source {
                    UV_REFERENCES
                        .iter()
                        .find(|r| r.source == source)
                        .map_or(region.nominal, |r| r.nominal)
                } else {
                    region.nominal
                },
                faces: region.faces,
                source_file: region.source,
                source_bone: region.bone,
                source_cube: region.cube,
                status: UvStatus::AuthoredCompatibility,
                mapping_note: if source == "villager_v2" {
                    "Published villager_v2 UV offsets; authored64x64 compatibility nominal; measured Jicklus/Whimscape64x64 and F8thful32x32 entity atlases; not a native nominal claim"
                } else if source == "sheep" {
                    "Published sheep UV offsets; authored64x32 compatibility nominal; measured Jicklus/Whimscape64x32 and F8thful32x16 entity atlases; not a native nominal claim"
                } else if region.source != source {
                    "Authored family compatibility: recorded villager source region in derived humanoid declared atlas; original inherited base geometry unresolved; not native part UV"
                } else {
                    "Authored anatomy reuses recorded anatomical source region; not native part UV; actual pack atlas identity/dimensions recorded on model"
                },
            });
        }
        let uv = part.uv.ok_or(CompatibilityError::MissingSourcedRegion)?;
        if uv
            .faces
            .iter()
            .any(|face| face.pixels(uv.nominal, atlas.png_dimensions).is_none())
        {
            return Err(CompatibilityError::InvalidUv);
        }
    }
    if matched {
        model.parts = parts;
        // Keep exact evidence for each semantic (e.g. a charged aura has a distinct
        // actual atlas from the core). Repeating a pure mapping is idempotent.
        if !has_active_evidence {
            model.atlas_evidence.push(atlas.clone());
        }
    }
    Ok(if matched {
        CompatibilityOutcome::Matched
    } else {
        CompatibilityOutcome::Unmatched
    })
}

#[cfg(test)]
mod compatibility_tests {
    use super::*;
    #[test]
    fn authored_compatibility_maps_all_static_anatomy_but_preserves_native_reference_counts() {
        for species in ALL_SPECIES {
            for layout in [
                AtlasLayout::Legacy,
                AtlasLayout::Modern,
                AtlasLayout::Bedrock,
            ] {
                for climate in [ClimateSkin::Temperate, ClimateSkin::Warm, ClimateSkin::Cold] {
                    let mut m = model(*species, layout, climate);
                    let old = m.parts.iter().filter(|p| p.uv.is_some()).count();
                    // Synthetic dimensions are a pure bounds fixture, not a claim this species
                    // exists in any particular pack. Real decoder evidence is mandatory in use.
                    let atlas = AtlasEvidence {
                        semantic: m.texture_semantic.into(),
                        resource_id: "test:synthetic-uv-bound-fixture".into(),
                        png_dimensions: [128, 64],
                        encoded_sha256: "0".repeat(64),
                    };
                    apply_authored_compatibility(&mut m, &atlas).unwrap_or_else(|error| {
                        panic!("{species:?}/{layout:?}/{climate:?}: {error:?}")
                    });
                    assert_eq!(m.unsupported_parts(), 0, "{species:?}");
                    assert_eq!(
                        m.parts
                            .iter()
                            .filter_map(|p| p.uv)
                            .filter(|u| u.status != UvStatus::AuthoredCompatibility)
                            .count(),
                        old
                    );
                    for p in m.parts {
                        let u = p.uv.unwrap();
                        for r in u.faces {
                            assert!(r.pixels(u.nominal, atlas.png_dimensions).is_some());
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn villager_authored_nominal_policy_scales_to_measured_actual_entity_atlases() {
        for (pack, dims, sha) in [
            (
                "F8thful",
                [32, 32],
                "fb975df9318a72ff571aa73d83809b85a9290a1fd330bf6492ab07527ee7b746",
            ),
            (
                "Jicklus",
                [64, 64],
                "8461767b9a1bd48d391875637e21f95e46a21ee35aae84e28c7a7bca235270e9",
            ),
        ] {
            let mut m = model(
                Species::Villager,
                AtlasLayout::Modern,
                ClimateSkin::Temperate,
            );
            let evidence = AtlasEvidence {
                semantic: m.texture_semantic.into(),
                resource_id: format!(
                    "{pack}:assets/minecraft/textures/entity/villager/villager.png"
                ),
                png_dimensions: dims,
                encoded_sha256: sha.into(),
            };
            apply_authored_compatibility(&mut m, &evidence).unwrap();
            assert!(m
                .parts
                .iter()
                .filter_map(|p| p.uv)
                .all(|u| u.status == UvStatus::AuthoredCompatibility && u.nominal == [64, 64]));
            assert_eq!(m.atlas_evidence[0], evidence);
        }
    }
    #[test]
    fn missing_invalid_or_wrong_semantic_evidence_never_quietly_maps_unrelated_pixels() {
        let mut m = model(Species::Cow, AtlasLayout::Modern, ClimateSkin::Temperate);
        let before = m.unsupported_parts();
        let mut evidence = AtlasEvidence {
            semantic: "minecraft:creeper/charged".into(),
            resource_id: "test:synthetic".into(),
            png_dimensions: [32, 32],
            encoded_sha256: "0".repeat(64),
        };
        let unchanged = format!("{:?}", m);
        assert_eq!(
            apply_authored_compatibility(&mut m, &evidence),
            Ok(CompatibilityOutcome::Unmatched)
        );
        assert_eq!(format!("{:?}", m), unchanged);
        assert!(m.atlas_evidence.is_empty());
        assert_eq!(m.unsupported_parts(), before);
        evidence.png_dimensions = [0, 32];
        assert_eq!(
            apply_authored_compatibility(&mut m, &evidence),
            Err(CompatibilityError::InvalidEvidence)
        );
    }
    #[test]
    fn conflicting_same_semantic_evidence_is_rejected_atomically_and_repeats_are_idempotent() {
        let mut m = model(Species::Cow, AtlasLayout::Modern, ClimateSkin::Temperate);
        let original = AtlasEvidence {
            semantic: m.texture_semantic.into(),
            resource_id: "test:synthetic-first".into(),
            png_dimensions: [64, 64],
            encoded_sha256: "1".repeat(64),
        };
        apply_authored_compatibility(&mut m, &original).unwrap();
        let before = format!("{:?}", m);
        apply_authored_compatibility(&mut m, &original).unwrap();
        assert_eq!(format!("{:?}", m), before);
        assert_eq!(m.atlas_evidence.len(), 1);
        let different = AtlasEvidence {
            resource_id: "test:synthetic-second".into(),
            ..original.clone()
        };
        assert_eq!(
            apply_authored_compatibility(&mut m, &different),
            Err(CompatibilityError::ConflictingEvidence)
        );
        assert_eq!(format!("{:?}", m), before);
        let dimensions = AtlasEvidence {
            png_dimensions: [128, 128],
            ..original.clone()
        };
        assert_eq!(
            apply_authored_compatibility(&mut m, &dimensions),
            Err(CompatibilityError::ConflictingEvidence)
        );
        assert_eq!(format!("{:?}", m), before);
        let hash = AtlasEvidence {
            encoded_sha256: "2".repeat(64),
            ..original
        };
        assert_eq!(
            apply_authored_compatibility(&mut m, &hash),
            Err(CompatibilityError::ConflictingEvidence)
        );
        assert_eq!(format!("{:?}", m), before);
    }
    #[test]
    fn late_invalid_uv_rejection_preserves_earlier_unmapped_parts_and_evidence() {
        let mut m = model(Species::Cow, AtlasLayout::Modern, ClimateSkin::Temperate);
        let unmapped = m.parts.iter().position(|p| p.uv.is_none()).unwrap();
        let mut invalid = m.parts[0].uv.unwrap();
        invalid.nominal = [0, 64];
        let mut late = m.parts[0].clone();
        late.uv = Some(invalid);
        m.parts.push(late);
        let before = format!("{:?}", m);
        let evidence = AtlasEvidence {
            semantic: m.texture_semantic.into(),
            resource_id: "test:synthetic-atomicity".into(),
            png_dimensions: [64, 64],
            encoded_sha256: "3".repeat(64),
        };
        assert_eq!(
            apply_authored_compatibility(&mut m, &evidence),
            Err(CompatibilityError::InvalidUv)
        );
        assert_eq!(format!("{:?}", m), before);
        assert!(m.parts[unmapped].uv.is_none());
        assert!(m.atlas_evidence.is_empty());
    }
}
