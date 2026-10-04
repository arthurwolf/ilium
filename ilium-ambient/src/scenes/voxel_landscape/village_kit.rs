//! Original village components, grounded by the five Java surface styles.
//! Geometry and mature display crops are authored homage, not copied templates
//! or native jigsaw/RNG algorithms. Assembly, terrain fit and assets are external.
use super::surface_structures::{PlacementError, Template, TemplateCell};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VillageStyle {
    Plains,
    Desert,
    Savanna,
    Taiga,
    Snowy,
}
impl VillageStyle {
    pub const ALL: [Self; 5] = [
        Self::Plains,
        Self::Desert,
        Self::Savanna,
        Self::Taiga,
        Self::Snowy,
    ];
    pub const fn name(self) -> &'static str {
        match self {
            Self::Plains => "plains",
            Self::Desert => "desert",
            Self::Savanna => "savanna",
            Self::Taiga => "taiga",
            Self::Snowy => "snowy",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HomeForm {
    Cottage,
    Longhouse,
    Tower,
    Courtyard,
}
impl HomeForm {
    pub const ALL: [Self; 4] = [Self::Cottage, Self::Longhouse, Self::Tower, Self::Courtyard];
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FarmForm {
    Compact,
    Long,
}
impl FarmForm {
    pub const ALL: [Self; 2] = [Self::Compact, Self::Long];
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoadShape {
    Straight,
    Corner,
    Tee,
    Cross,
    Slope,
    Terminator,
}
impl RoadShape {
    pub const ALL: [Self; 6] = [
        Self::Straight,
        Self::Corner,
        Self::Tee,
        Self::Cross,
        Self::Slope,
        Self::Terminator,
    ];
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Profession {
    Armorer,
    Butcher,
    Cartographer,
    Cleric,
    Farmer,
    Fisherman,
    Fletcher,
    Leatherworker,
    Librarian,
    Mason,
    Shepherd,
    Toolsmith,
    Weaponsmith,
}
impl Profession {
    pub const ALL: [Self; 13] = [
        Self::Armorer,
        Self::Butcher,
        Self::Cartographer,
        Self::Cleric,
        Self::Farmer,
        Self::Fisherman,
        Self::Fletcher,
        Self::Leatherworker,
        Self::Librarian,
        Self::Mason,
        Self::Shepherd,
        Self::Toolsmith,
        Self::Weaponsmith,
    ];
    pub const fn workstation(self) -> &'static str {
        match self {
            Self::Armorer => "minecraft:blast_furnace",
            Self::Butcher => "minecraft:smoker",
            Self::Cartographer => "minecraft:cartography_table",
            Self::Cleric => "minecraft:brewing_stand",
            Self::Farmer => "minecraft:composter",
            Self::Fisherman => "minecraft:barrel",
            Self::Fletcher => "minecraft:fletching_table",
            Self::Leatherworker => "minecraft:cauldron",
            Self::Librarian => "minecraft:lectern",
            Self::Mason => "minecraft:stonecutter",
            Self::Shepherd => "minecraft:loom",
            Self::Toolsmith => "minecraft:smithing_table",
            Self::Weaponsmith => "minecraft:grindstone",
        }
    }
    fn properties(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::Armorer | Self::Butcher => &[("facing", "north"), ("lit", "false")],
            Self::Cleric => &[
                ("has_bottle_0", "false"),
                ("has_bottle_1", "false"),
                ("has_bottle_2", "false"),
            ],
            Self::Farmer => &[("level", "0")],
            Self::Fisherman => &[("facing", "up"), ("open", "false")],
            Self::Librarian => &[
                ("facing", "north"),
                ("has_book", "false"),
                ("powered", "false"),
            ],
            Self::Mason | Self::Shepherd => &[("facing", "north")],
            Self::Weaponsmith => &[("face", "floor"), ("facing", "north")],
            _ => &[],
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PieceKind {
    Home(HomeForm),
    Workplace(Profession),
    Farm(FarmForm),
    Pen,
    Center,
    Road(RoadShape),
    Lamp,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortRole {
    Street,
    Entrance,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Port {
    pub position: [i32; 3],
    pub front: [i32; 2],
    pub role: PortRole,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkerKind {
    Resident(Option<Profession>),
    Animal(&'static str),
    /// Original static plaza guardian, not native golem spawning gameplay.
    Guardian,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Marker {
    pub position: [i32; 3],
    pub kind: MarkerKind,
}
pub struct Piece<S> {
    pub template: Template<S>,
    /// Inclusive lower and exclusive upper extent, including overhang and air.
    pub bounds: [[i32; 3]; 2],
    pub ports: Vec<Port>,
    pub markers: Vec<Marker>,
    /// Authored traversable floor coordinates; assembly checks actual headroom.
    pub walkable: Vec<[i32; 3]>,
}

#[derive(Clone, Copy)]
struct Palette {
    wall: &'static str,
    floor: &'static str,
    log: &'static str,
    stairs: &'static str,
    slab: &'static str,
    door: &'static str,
    fence: &'static str,
    gate: &'static str,
}
fn palette(style: VillageStyle) -> Palette {
    match style {
        VillageStyle::Plains => Palette {
            wall: "minecraft:oak_planks",
            floor: "minecraft:cobblestone",
            log: "minecraft:oak_log",
            stairs: "minecraft:oak_stairs",
            slab: "minecraft:oak_slab",
            door: "minecraft:oak_door",
            fence: "minecraft:oak_fence",
            gate: "minecraft:oak_fence_gate",
        },
        VillageStyle::Desert => Palette {
            wall: "minecraft:smooth_sandstone",
            floor: "minecraft:cut_sandstone",
            log: "minecraft:cut_sandstone",
            stairs: "minecraft:sandstone_stairs",
            slab: "minecraft:sandstone_slab",
            door: "minecraft:jungle_door",
            fence: "minecraft:jungle_fence",
            gate: "minecraft:jungle_fence_gate",
        },
        VillageStyle::Savanna => Palette {
            wall: "minecraft:orange_terracotta",
            floor: "minecraft:acacia_planks",
            log: "minecraft:acacia_log",
            stairs: "minecraft:acacia_stairs",
            slab: "minecraft:acacia_slab",
            door: "minecraft:acacia_door",
            fence: "minecraft:acacia_fence",
            gate: "minecraft:acacia_fence_gate",
        },
        VillageStyle::Taiga => Palette {
            wall: "minecraft:spruce_planks",
            floor: "minecraft:mossy_cobblestone",
            log: "minecraft:spruce_log",
            stairs: "minecraft:spruce_stairs",
            slab: "minecraft:spruce_slab",
            door: "minecraft:spruce_door",
            fence: "minecraft:spruce_fence",
            gate: "minecraft:spruce_fence_gate",
        },
        VillageStyle::Snowy => Palette {
            wall: "minecraft:snow_block",
            floor: "minecraft:spruce_planks",
            log: "minecraft:stripped_spruce_log",
            stairs: "minecraft:spruce_stairs",
            slab: "minecraft:spruce_slab",
            door: "minecraft:spruce_door",
            fence: "minecraft:spruce_fence",
            gate: "minecraft:spruce_fence_gate",
        },
    }
}
type StateFactory<'a, S> = dyn Fn(&str, &[(&str, &str)]) -> Result<S, PlacementError> + 'a;
struct Writer<'a, S> {
    factory: &'a StateFactory<'a, S>,
    cells: BTreeMap<[i32; 3], Option<S>>,
    ports: Vec<Port>,
    markers: Vec<Marker>,
    walkable: Vec<[i32; 3]>,
}
impl<S> Writer<'_, S> {
    fn put(&mut self, p: [i32; 3], id: &str, props: &[(&str, &str)]) -> Result<(), PlacementError> {
        self.cells.insert(p, Some((self.factory)(id, props)?));
        Ok(())
    }
    fn air(&mut self, p: [i32; 3]) {
        self.cells.insert(p, None);
    }
    fn fence(
        &mut self,
        p: [i32; 3],
        id: &str,
        connections: [bool; 4],
    ) -> Result<(), PlacementError> {
        self.put(
            p,
            id,
            &[
                ("north", if connections[0] { "true" } else { "false" }),
                ("east", if connections[1] { "true" } else { "false" }),
                ("south", if connections[2] { "true" } else { "false" }),
                ("west", if connections[3] { "true" } else { "false" }),
                ("waterlogged", "false"),
            ],
        )
    }
}
fn log<S>(w: &mut Writer<S>, p: [i32; 3], pal: Palette, axis: &str) -> Result<(), PlacementError> {
    let properties = [("axis", axis)];
    w.put(
        p,
        pal.log,
        if pal.log.ends_with("_log") {
            &properties
        } else {
            &[]
        },
    )
}
fn stair<S>(
    w: &mut Writer<S>,
    p: [i32; 3],
    pal: Palette,
    facing: &str,
) -> Result<(), PlacementError> {
    w.put(
        p,
        pal.stairs,
        &[
            ("facing", facing),
            ("half", "bottom"),
            ("shape", "straight"),
            ("waterlogged", "false"),
        ],
    )
}

fn house<S>(
    w: &mut Writer<S>,
    style: VillageStyle,
    form: HomeForm,
    profession: Option<Profession>,
) -> Result<(), PlacementError> {
    let pal = palette(style);
    let (width, length, height) = match form {
        HomeForm::Cottage => (7, 7, 4),
        HomeForm::Longhouse => (9, 11, 4),
        HomeForm::Tower => (7, 7, 8),
        HomeForm::Courtyard => (11, 11, 4),
    };
    for x in 0..width {
        for y in 0..length {
            w.put([x, y, 0], pal.floor, &[])?;
            for z in 1..=height {
                w.air([x, y, z]);
                if x == 0 || x == width - 1 || y == 0 || y == length - 1 {
                    w.put([x, y, z], pal.wall, &[])?;
                }
            }
        }
    }
    for x in [0, width - 1] {
        for y in [0, length - 1] {
            for z in 1..=height {
                log(w, [x, y, z], pal, "y")?;
            }
        }
    }
    for y in [2, length - 3] {
        for x in [0, width - 1] {
            if style == VillageStyle::Desert {
                w.air([x, y, 2]);
                w.put(
                    [x, y, 3],
                    pal.slab,
                    &[("type", "top"), ("waterlogged", "false")],
                )?;
                continue;
            }
            w.put(
                [x, y, 2],
                "minecraft:glass_pane",
                &[
                    ("north", "true"),
                    ("east", "false"),
                    ("south", "true"),
                    ("west", "false"),
                    ("waterlogged", "false"),
                ],
            )?;
        }
    }
    let door = width / 2;
    for (z, half) in [(1, "lower"), (2, "upper")] {
        w.put(
            [door, 0, z],
            pal.door,
            &[
                ("facing", "north"),
                ("half", half),
                ("hinge", "left"),
                ("open", "false"),
                ("powered", "false"),
            ],
        )?;
    }
    stair(w, [door, -1, 0], pal, "south")?;
    w.air([door, -1, 1]);
    w.air([door, -1, 2]);
    w.ports.push(Port {
        position: [door, -1, 0],
        front: [0, -1],
        role: PortRole::Entrance,
    });
    for y in -1..length - 1 {
        w.walkable.push([door, y, 0]);
    }
    for x in -1..=width {
        for y in -1..=length {
            if form == HomeForm::Courtyard && (4..=6).contains(&x) && (4..=6).contains(&y) {
                continue;
            }
            let rise = match style {
                VillageStyle::Desert => 0,
                VillageStyle::Savanna => x.min(width - 1 - x).max(0) / 2,
                _ => x.min(width - 1 - x).max(0),
            };
            let roof = height + 1 + rise;
            if style == VillageStyle::Desert {
                w.put([x, y, roof], pal.wall, &[])?;
                if x == -1 || x == width || y == -1 || y == length {
                    w.put(
                        [x, y, roof + 1],
                        pal.slab,
                        &[("type", "bottom"), ("waterlogged", "false")],
                    )?;
                }
            } else {
                stair(
                    w,
                    [x, y, roof],
                    pal,
                    if x < width / 2 { "east" } else { "west" },
                )?;
                if style == VillageStyle::Snowy {
                    w.put([x, y, roof + 1], "minecraft:snow", &[("layers", "1")])?;
                }
            }
        }
    }
    // Close pitched gables so the roof is a building shell, not floating strips.
    if style != VillageStyle::Desert {
        for x in 1..width - 1 {
            let rise = if style == VillageStyle::Savanna {
                x.min(width - 1 - x) / 2
            } else {
                x.min(width - 1 - x)
            };
            for y in [0, length - 1] {
                for z in height + 1..height + 1 + rise {
                    w.put([x, y, z], pal.wall, &[])?;
                }
            }
        }
    }
    if form == HomeForm::Courtyard {
        for x in 4..=6 {
            for y in 4..=6 {
                w.put([x, y, 0], "minecraft:grass_block", &[("snowy", "false")])?;
            }
        }
        w.put([5, 5, 1], "minecraft:poppy", &[])?;
    }
    for (y, part) in [(length - 3, "foot"), (length - 2, "head")] {
        w.put(
            [1, y, 1],
            "minecraft:red_bed",
            &[("facing", "south"), ("part", part), ("occupied", "false")],
        )?;
    }
    w.put(
        [width - 2, length - 2, 1],
        "minecraft:chest",
        &[
            ("facing", "north"),
            ("type", "single"),
            ("waterlogged", "false"),
        ],
    )?;
    w.put([1, 1, 1], "minecraft:crafting_table", &[])?;
    w.markers.push(Marker {
        position: [door, 2, 1],
        kind: MarkerKind::Resident(profession),
    });
    if let Some(job) = profession {
        workplace(w, style, pal, job, width, length, height)?;
    }
    Ok(())
}

fn workplace<S>(
    w: &mut Writer<S>,
    style: VillageStyle,
    pal: Palette,
    job: Profession,
    width: i32,
    length: i32,
    height: i32,
) -> Result<(), PlacementError> {
    w.put([width - 2, 2, 1], job.workstation(), job.properties())?;
    match job {
        Profession::Armorer
        | Profession::Butcher
        | Profession::Toolsmith
        | Profession::Weaponsmith => {
            for z in 1..=height + 3 {
                w.put([width - 2, length - 2, z], "minecraft:cobblestone", &[])?;
            }
            w.put(
                [width - 2, length - 2, 1],
                "minecraft:furnace",
                &[("facing", "north"), ("lit", "false")],
            )?;
            w.put([width - 3, 2, 1], "minecraft:anvil", &[("facing", "east")])?;
        }
        Profession::Librarian | Profession::Cartographer => {
            for x in 1..width - 1 {
                for z in 1..=2 {
                    w.put([x, length - 1, z], "minecraft:bookshelf", &[])?;
                }
            }
        }
        Profession::Cleric => {
            w.put([width / 2, length - 2, 1], "minecraft:yellow_carpet", &[])?;
            w.put(
                [width / 2, length - 1, height],
                if style == VillageStyle::Desert {
                    "minecraft:yellow_terracotta"
                } else {
                    "minecraft:yellow_stained_glass"
                },
                &[],
            )?;
        }
        Profession::Fisherman => {
            for y in 2..4 {
                w.put(
                    [1, y, 1],
                    "minecraft:barrel",
                    &[("facing", "up"), ("open", "false")],
                )?;
            }
        }
        Profession::Leatherworker => {
            w.put([1, 2, 1], "minecraft:water_cauldron", &[("level", "3")])?;
        }
        Profession::Shepherd => {
            for (x, id) in [
                (1, "minecraft:white_wool"),
                (2, "minecraft:yellow_wool"),
                (3, "minecraft:red_wool"),
            ] {
                w.put([x, 2, 1], id, &[])?;
            }
        }
        Profession::Mason => {
            for y in 2..4 {
                w.put([1, y, 1], "minecraft:stone_bricks", &[])?;
            }
        }
        Profession::Farmer => {
            w.put([1, 2, 1], "minecraft:hay_block", &[("axis", "y")])?;
        }
        Profession::Fletcher => {
            log(w, [1, 2, 1], pal, "x")?;
            w.put([1, 3, 1], "minecraft:target", &[("power", "0")])?;
        }
    }
    Ok(())
}

fn entropy(seed: u64, x: i32, y: i32, lane: u64) -> f64 {
    let mut v = seed
        ^ (x as u64).wrapping_mul(0x9e3779b97f4a7c15)
        ^ (y as u64).wrapping_mul(0xbf58476d1ce4e5b9)
        ^ lane;
    v = (v ^ (v >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    v = (v ^ (v >> 27)).wrapping_mul(0x94d049bb133111eb);
    ((v ^ (v >> 31)) >> 11) as f64 / (1u64 << 53) as f64
}
fn crop(style: VillageStyle, seed: u64, x: i32, y: i32) -> (&'static str, &'static str) {
    let rules: &[(&str, f64, &str)] = match style {
        VillageStyle::Desert => &[
            ("minecraft:beetroots", 0.2, "3"),
            ("minecraft:melon_stem", 0.1, "7"),
        ],
        VillageStyle::Plains => &[
            ("minecraft:carrots", 0.3, "7"),
            ("minecraft:potatoes", 0.2, "7"),
            ("minecraft:beetroots", 0.1, "3"),
        ],
        VillageStyle::Snowy => &[
            ("minecraft:carrots", 0.1, "7"),
            ("minecraft:potatoes", 0.8, "7"),
        ],
        VillageStyle::Taiga => &[
            ("minecraft:pumpkin_stem", 0.3, "7"),
            ("minecraft:potatoes", 0.2, "7"),
        ],
        VillageStyle::Savanna => &[("minecraft:melon_stem", 0.1, "7")],
    };
    for (i, (id, chance, age)) in rules.iter().enumerate() {
        if entropy(seed, x, y, i as u64) < *chance {
            return (id, age);
        }
    }
    ("minecraft:wheat", "7")
}
fn farm<S>(
    w: &mut Writer<S>,
    style: VillageStyle,
    form: FarmForm,
    seed: u64,
) -> Result<(), PlacementError> {
    let pal = palette(style);
    let length = if form == FarmForm::Compact { 7 } else { 13 };
    for x in 0..9 {
        for y in 0..length {
            w.put([x, y, -1], "minecraft:dirt", &[])?;
            w.air([x, y, 1]);
            w.air([x, y, 2]);
            if x == 0 || x == 8 || y == 0 || y == length - 1 {
                log(
                    w,
                    [x, y, 0],
                    pal,
                    if y == 0 || y == length - 1 { "x" } else { "z" },
                )?;
            } else if x == 4 {
                w.put([x, y, 0], "minecraft:water", &[("level", "0")])?;
            } else {
                w.put([x, y, 0], "minecraft:farmland", &[("moisture", "7")])?;
                let (id, age) = crop(style, seed, x, y);
                w.put([x, y, 1], id, &[("age", age)])?;
            }
        }
    }
    w.put([4, 0, 0], pal.floor, &[])?;
    w.put([4, length - 1, 0], pal.floor, &[])?;
    w.put([1, 0, 1], "minecraft:composter", &[("level", "0")])?;
    w.ports.push(Port {
        position: [4, 0, 0],
        front: [0, -1],
        role: PortRole::Entrance,
    });
    w.walkable.push([4, 0, 0]);
    w.markers.push(Marker {
        position: [3, 0, 1],
        kind: MarkerKind::Resident(Some(Profession::Farmer)),
    });
    Ok(())
}
fn pen<S>(w: &mut Writer<S>, style: VillageStyle) -> Result<(), PlacementError> {
    let pal = palette(style);
    for x in 0..7 {
        for y in 0..9 {
            w.put([x, y, 0], "minecraft:grass_block", &[("snowy", "false")])?;
            for z in 1..=3 {
                w.air([x, y, z]);
            }
            if x == 0 || x == 6 || y == 0 || y == 8 {
                w.fence(
                    [x, y, 1],
                    pal.fence,
                    [
                        (x == 0 || x == 6) && y > 0,
                        (y == 0 || y == 8) && x < 6,
                        (x == 0 || x == 6) && y < 8,
                        (y == 0 || y == 8) && x > 0,
                    ],
                )?;
            }
        }
    }
    w.put(
        [3, 0, 1],
        pal.gate,
        &[
            ("facing", "north"),
            ("in_wall", "false"),
            ("open", "true"),
            ("powered", "false"),
        ],
    )?;
    w.ports.push(Port {
        position: [3, 0, 0],
        front: [0, -1],
        role: PortRole::Entrance,
    });
    for y in 0..8 {
        w.walkable.push([3, y, 0]);
    }
    w.markers.push(Marker {
        position: [2, 4, 1],
        kind: MarkerKind::Animal("minecraft:cow"),
    });
    w.markers.push(Marker {
        position: [4, 5, 1],
        kind: MarkerKind::Animal("minecraft:sheep"),
    });
    Ok(())
}
fn center<S>(w: &mut Writer<S>, style: VillageStyle) -> Result<(), PlacementError> {
    let pal = palette(style);
    for x in 0..11 {
        for y in 0..11 {
            w.put([x, y, 0], pal.floor, &[])?;
            w.air([x, y, 1]);
            w.air([x, y, 2]);
            let fountain = (3..=7).contains(&x) && (3..=7).contains(&y);
            let bell_post = y == 9 && [4, 6].contains(&x);
            if !(fountain || bell_post) {
                w.walkable.push([x, y, 0]);
            }
        }
    }
    for x in 3..=7 {
        for y in 3..=7 {
            if x == 3 || x == 7 || y == 3 || y == 7 {
                w.put([x, y, 1], pal.wall, &[])?;
            } else {
                w.put(
                    [x, y, 1],
                    if style == VillageStyle::Snowy {
                        "minecraft:ice"
                    } else {
                        "minecraft:water"
                    },
                    if style == VillageStyle::Snowy {
                        &[]
                    } else {
                        &[("level", "0")]
                    },
                )?;
            }
        }
    }
    for x in [4, 6] {
        for z in 1..=3 {
            log(w, [x, 9, z], pal, "y")?;
        }
    }
    for x in 4..=6 {
        log(w, [x, 9, 4], pal, "x")?;
    }
    w.put(
        [5, 9, 3],
        "minecraft:bell",
        &[
            ("attachment", "ceiling"),
            ("facing", "north"),
            ("powered", "false"),
        ],
    )?;
    for (position, front) in [
        ([5, 0, 0], [0, -1]),
        ([10, 5, 0], [1, 0]),
        ([5, 10, 0], [0, 1]),
        ([0, 5, 0], [-1, 0]),
    ] {
        w.ports.push(Port {
            position,
            front,
            role: PortRole::Street,
        });
    }
    // The broad original golem needs three blocks of carved headroom across
    // its whole shoulder footprint. Keep the fountain, bell and street ports.
    for x in 7..=9 {
        w.air([x, 2, 3]);
    }
    w.markers.push(Marker {
        position: [8, 2, 1],
        kind: MarkerKind::Guardian,
    });
    w.markers.push(Marker {
        position: [2, 2, 1],
        kind: if style == VillageStyle::Desert {
            MarkerKind::Animal("minecraft:camel")
        } else {
            MarkerKind::Resident(None)
        },
    });
    Ok(())
}
fn road<S>(w: &mut Writer<S>, style: VillageStyle, shape: RoadShape) -> Result<(), PlacementError> {
    let pal = palette(style);
    let arms = match shape {
        RoadShape::Straight | RoadShape::Slope => [true, false, true, false],
        RoadShape::Corner => [true, true, false, false],
        RoadShape::Tee => [true, true, false, true],
        RoadShape::Cross => [true; 4],
        RoadShape::Terminator => [true, false, false, false],
    };
    for x in 0..11 {
        for y in 0..11 {
            let core = (4..=6).contains(&x) && (4..=6).contains(&y);
            let active = core
                || (arms[0] && (4..=6).contains(&x) && y < 5)
                || (arms[2] && (4..=6).contains(&x) && y > 5)
                || (arms[1] && (4..=6).contains(&y) && x > 5)
                || (arms[3] && (4..=6).contains(&y) && x < 5);
            if !active {
                continue;
            }
            let z = if shape == RoadShape::Slope && y >= 6 {
                1
            } else {
                0
            };
            if shape == RoadShape::Slope && y == 5 {
                stair(w, [x, y, z], pal, "south")?;
            } else {
                w.put(
                    [x, y, z],
                    if style == VillageStyle::Desert {
                        pal.floor
                    } else {
                        "minecraft:dirt_path"
                    },
                    &[],
                )?;
            }
            w.air([x, y, z + 1]);
            w.air([x, y, z + 2]);
            w.walkable.push([x, y, z]);
        }
    }
    for (i, (position, front)) in [
        ([5, 0, 0], [0, -1]),
        ([10, 5, 0], [1, 0]),
        (
            [5, 10, if shape == RoadShape::Slope { 1 } else { 0 }],
            [0, 1],
        ),
        ([0, 5, 0], [-1, 0]),
    ]
    .into_iter()
    .enumerate()
    {
        if arms[i] {
            w.ports.push(Port {
                position,
                front,
                role: PortRole::Street,
            });
        }
    }
    Ok(())
}
fn lamp<S>(w: &mut Writer<S>, style: VillageStyle) -> Result<(), PlacementError> {
    let pal = palette(style);
    w.put([0, 0, 0], pal.floor, &[])?;
    for z in 1..4 {
        w.fence([0, 0, z], pal.fence, [false; 4])?;
    }
    log(w, [0, 0, 4], pal, "y")?;
    w.put(
        [0, 0, 5],
        pal.slab,
        &[("type", "bottom"), ("waterlogged", "false")],
    )?;
    for (x, y, facing) in [
        (0, -1, "north"),
        (1, 0, "east"),
        (0, 1, "south"),
        (-1, 0, "west"),
    ] {
        w.put([x, y, 4], "minecraft:wall_torch", &[("facing", facing)])?;
    }
    Ok(())
}

/// An original component. The state adapter supplies exact namespaced identity;
/// a failed adapter rejects the whole piece. Deliberate local shell/furniture
/// overwrites are resolved here; the final sparse template has unique positions.
pub fn build<S>(
    style: VillageStyle,
    kind: PieceKind,
    seed: u64,
    factory: impl Fn(&str, &[(&str, &str)]) -> Result<S, PlacementError>,
) -> Result<Piece<S>, PlacementError> {
    let mut w = Writer {
        factory: &factory,
        cells: BTreeMap::new(),
        ports: vec![],
        markers: vec![],
        walkable: vec![],
    };
    match kind {
        PieceKind::Home(form) => house(&mut w, style, form, None)?,
        PieceKind::Workplace(job) => {
            let form = match job {
                Profession::Cleric => HomeForm::Tower,
                Profession::Librarian | Profession::Cartographer | Profession::Shepherd => {
                    HomeForm::Longhouse
                }
                Profession::Farmer => HomeForm::Courtyard,
                _ => HomeForm::Cottage,
            };
            house(&mut w, style, form, Some(job))?;
        }
        PieceKind::Farm(form) => farm(&mut w, style, form, seed)?,
        PieceKind::Pen => pen(&mut w, style)?,
        PieceKind::Center => center(&mut w, style)?,
        PieceKind::Road(shape) => road(&mut w, style, shape)?,
        PieceKind::Lamp => lamp(&mut w, style)?,
    }
    let mut lower = [i32::MAX; 3];
    let mut upper = [i32::MIN; 3];
    for p in w.cells.keys() {
        for axis in 0..3 {
            lower[axis] = lower[axis].min(p[axis]);
            upper[axis] = upper[axis].max(p[axis] + 1);
        }
    }
    Ok(Piece {
        template: Template {
            source: format!("homage:village/{}/{kind:?}/seed{seed}", style.name()),
            cells: w
                .cells
                .into_iter()
                .map(|(position, state)| TemplateCell { position, state })
                .collect(),
        },
        bounds: [lower, upper],
        ports: w.ports,
        markers: w.markers,
        walkable: w.walkable,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    type State = (String, BTreeMap<String, String>);
    fn state(id: &str, properties: &[(&str, &str)]) -> Result<State, PlacementError> {
        Ok((
            id.to_owned(),
            properties
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        ))
    }

    #[test]
    fn five_styles_have_geometry_and_complete_facility_components() {
        for style in VillageStyle::ALL {
            let mut extents = BTreeSet::new();
            for home in HomeForm::ALL {
                let piece = build(style, PieceKind::Home(home), 97, state).unwrap();
                extents.insert(piece.bounds);
                assert_eq!(piece.ports.len(), 1);
                assert!(piece
                    .template
                    .cells
                    .iter()
                    .any(|cell| cell.state.as_ref().is_some_and(|s| s.0.ends_with("_bed"))));
            }
            assert_eq!(extents.len(), 4);
            for job in Profession::ALL {
                let piece = build(style, PieceKind::Workplace(job), 97, state).unwrap();
                assert!(piece
                    .markers
                    .iter()
                    .any(|m| m.kind == MarkerKind::Resident(Some(job))));
                assert!(piece.template.cells.iter().any(|cell| cell
                    .state
                    .as_ref()
                    .is_some_and(|s| s.0 == job.workstation())));
            }
            for farm in FarmForm::ALL {
                let piece = build(style, PieceKind::Farm(farm), 97, state).unwrap();
                for id in [
                    "minecraft:farmland",
                    "minecraft:water",
                    "minecraft:composter",
                ] {
                    assert!(piece
                        .template
                        .cells
                        .iter()
                        .any(|cell| cell.state.as_ref().is_some_and(|s| s.0 == id)));
                }
            }
            for kind in [PieceKind::Pen, PieceKind::Center, PieceKind::Lamp]
                .into_iter()
                .chain(RoadShape::ALL.into_iter().map(PieceKind::Road))
            {
                assert!(!build(style, kind, 97, state)
                    .unwrap()
                    .template
                    .cells
                    .is_empty());
            }
        }
    }

    #[test]
    fn every_bed_and_door_half_has_its_matching_partner() {
        for style in VillageStyle::ALL {
            for kind in HomeForm::ALL
                .into_iter()
                .map(PieceKind::Home)
                .chain(Profession::ALL.into_iter().map(PieceKind::Workplace))
            {
                let piece = build(style, kind, 11, state).unwrap();
                let cells: BTreeMap<_, _> = piece
                    .template
                    .cells
                    .iter()
                    .map(|c| (c.position, &c.state))
                    .collect();
                for (position, state) in &cells {
                    let Some((id, properties)) = state else {
                        continue;
                    };
                    if id.ends_with("_bed") {
                        let step = if properties["part"] == "foot" { 1 } else { -1 };
                        let partner = cells[&[position[0], position[1] + step, position[2]]]
                            .as_ref()
                            .unwrap();
                        assert_eq!(&partner.0, id, "{style:?}/{kind:?} orphan bed");
                        assert_ne!(partner.1["part"], properties["part"]);
                        assert_eq!(partner.1["facing"], properties["facing"]);
                    }
                    if id.ends_with("_door") {
                        let step = if properties["half"] == "lower" { 1 } else { -1 };
                        let partner = cells[&[position[0], position[1], position[2] + step]]
                            .as_ref()
                            .unwrap();
                        assert_eq!(&partner.0, id);
                        assert_ne!(partner.1["half"], properties["half"]);
                        assert_eq!(partner.1["facing"], properties["facing"]);
                    }
                }
            }
        }
    }

    #[test]
    fn connectors_have_support_and_pieces_are_stable_and_bounded() {
        for style in VillageStyle::ALL {
            for kind in [
                PieceKind::Home(HomeForm::Courtyard),
                PieceKind::Farm(FarmForm::Long),
                PieceKind::Pen,
                PieceKind::Center,
            ]
            .into_iter()
            .chain(RoadShape::ALL.into_iter().map(PieceKind::Road))
            {
                let a = build(style, kind, 8, state).unwrap();
                let b = build(style, kind, 8, state).unwrap();
                let cells: BTreeMap<_, _> = a
                    .template
                    .cells
                    .iter()
                    .map(|c| (c.position, &c.state))
                    .collect();
                assert_eq!(cells.len(), a.template.cells.len());
                assert_eq!(
                    cells,
                    b.template
                        .cells
                        .iter()
                        .map(|c| (c.position, &c.state))
                        .collect()
                );
                assert!(cells.len() < super::super::surface_structures::MAX_CELLS);
                assert!(cells.keys().all(|p| p
                    .iter()
                    .all(|v| v.abs() < super::super::surface_structures::MAX_OFFSET)));
                for port in &a.ports {
                    assert!(cells.get(&port.position).is_some_and(|s| s.is_some()));
                }
            }
        }
    }

    #[test]
    fn state_failure_rejects_the_entire_component() {
        assert!(
            build::<State>(VillageStyle::Desert, PieceKind::Center, 0, |_, _| Err(
                PlacementError::InvalidState
            ))
            .is_err()
        );
    }

    #[test]
    fn desert_facades_and_lamp_supports_preserve_style_and_clearance() {
        for kind in HomeForm::ALL
            .into_iter()
            .map(PieceKind::Home)
            .chain(Profession::ALL.into_iter().map(PieceKind::Workplace))
        {
            let piece = build(VillageStyle::Desert, kind, 0, state).unwrap();
            assert!(!piece
                .template
                .cells
                .iter()
                .any(|c| c.state.as_ref().is_some_and(|s| s.0.contains("glass"))));
            assert!(piece.template.cells.iter().any(|c| c
                .state
                .as_ref()
                .is_some_and(|s| s.0 == "minecraft:jungle_door")));
        }
        for style in VillageStyle::ALL {
            let lamp = build(style, PieceKind::Lamp, 0, state).unwrap();
            let support = lamp
                .template
                .cells
                .iter()
                .find(|c| c.position == [0, 0, 4])
                .unwrap()
                .state
                .as_ref()
                .unwrap();
            assert!(!support.0.ends_with("_fence"));
            let center = build(style, PieceKind::Center, 0, state).unwrap();
            assert!(!center.walkable.contains(&[4, 9, 0]));
            assert!(!center.walkable.contains(&[6, 9, 0]));
        }
    }
}

#[cfg(test)]
mod guardian_tests {
    use super::super::surface_entities::{self, AtlasLayout, ClimateSkin, Species};
    use super::*;
    type State = (String, BTreeMap<String, String>);
    fn state(id: &str, props: &[(&str, &str)]) -> Result<State, PlacementError> {
        Ok((
            id.into(),
            props
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        ))
    }
    #[test]
    fn each_village_style_has_one_guardian_with_whole_body_clearance_and_support() {
        let model = surface_entities::model(
            Species::IronGolem,
            AtlasLayout::Bedrock,
            ClimateSkin::Temperate,
        );
        let (minimum, maximum) = model.bounds().unwrap();
        for style in VillageStyle::ALL {
            let kit = build(style, PieceKind::Center, 97, state).unwrap();
            let guardians: Vec<_> = kit
                .markers
                .iter()
                .filter(|marker| marker.kind == MarkerKind::Guardian)
                .collect();
            assert_eq!(guardians.len(), 1, "{style:?} has no single plaza guardian");
            let anchor = guardians[0].position;
            let cells: BTreeMap<_, _> = kit
                .template
                .cells
                .iter()
                .map(|cell| (cell.position, &cell.state))
                .collect();
            let lower: [i32; 3] = std::array::from_fn(|axis| {
                (anchor[axis] as f32 + if axis < 2 { 0.5 } else { 0.0 } + minimum[axis]).floor()
                    as i32
            });
            let upper: [i32; 3] = std::array::from_fn(|axis| {
                (anchor[axis] as f32 + if axis < 2 { 0.5 } else { 0.0 } + maximum[axis]).ceil()
                    as i32
            });
            for y in lower[1]..upper[1] {
                for x in lower[0]..upper[0] {
                    assert!(
                        cells.get(&[x, y, anchor[2] - 1]).unwrap().is_some(),
                        "Missing guardian floor {style:?}"
                    );
                    for z in lower[2]..upper[2] {
                        assert!(
                            cells.get(&[x, y, z]).is_some_and(|state| state.is_none()),
                            "Guardian intersects retained terrain or plaza {style:?} {:?}",
                            [x, y, z]
                        );
                    }
                }
            }
            assert!(
                kit.markers
                    .iter()
                    .filter(|marker| marker.kind != MarkerKind::Guardian)
                    .count()
                    >= 1
            );
            for kind in [
                PieceKind::Home(HomeForm::Cottage),
                PieceKind::Pen,
                PieceKind::Road(RoadShape::Cross),
            ] {
                assert!(!build(style, kind, 97, state)
                    .unwrap()
                    .markers
                    .iter()
                    .any(|marker| marker.kind == MarkerKind::Guardian));
            }
        }
    }
}

#[cfg(test)]
#[path = "village_kit_layout_tests.rs"]
mod village_layout_tests;
