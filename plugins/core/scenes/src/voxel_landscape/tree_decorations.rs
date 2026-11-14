//! Bounded semantic decoration placement on actual tree and surrounding cells.
//! Placement is an original homage; source probabilities are supplied by callers.
use super::tree_decoration_profiles::{DecorationProfile, DecoratorKind, DecoratorScope};
use super::tree_forms::TreeVoxelState;
use super::tree_geometry::{LogAxis, TreeCell, TreeGeometry};
use super::tree_profiles::TreeResource;
use std::collections::{BTreeMap, BTreeSet};

/// Unknown terrain is never treated as empty or replaceable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurroundingCell {
    Air,
    Solid,
    /// Caller-qualified membership in the active soil decorator's source tag.
    /// General diggable or merely solid ground does not qualify.
    ReplaceableSoil,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecorationOperation {
    PlaceInAir,
    ReplaceSoil,
    ReplaceLog,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decoration {
    pub state: TreeVoxelState,
    pub operation: DecorationOperation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ApplicationStatus {
    Applied {
        cells: usize,
        authored_parameters: &'static [&'static str],
    },
    Pending(&'static str),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecorationApplication {
    pub source_kind: &'static str,
    pub scope: DecoratorScope,
    pub status: ApplicationStatus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Attachment {
    BeeNest,
    TrunkVine,
    LeafVine,
    Cocoa,
    ShelfMushroom,
}

const SIDES: [([i16; 3], &str, &str); 4] = [
    ([1, 0, 0], "east", "west"),
    ([-1, 0, 0], "west", "east"),
    ([0, 1, 0], "south", "north"),
    ([0, -1, 0], "north", "south"),
];

fn entropy(seed: u64, position: [i16; 3], salt: u64) -> u64 {
    let mut hash = seed ^ salt;
    for coordinate in position {
        hash ^= (i64::from(coordinate) as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
        hash = hash.rotate_left(21).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    }
    hash ^ (hash >> 31)
}

fn selected(hash: u64, probability: f64) -> bool {
    probability.is_finite()
        && (0.0..=1.0).contains(&probability)
        && ((hash >> 11) as f64 / 9_007_199_254_740_992.0) < probability
}

fn choose_resource(resources: &[TreeResource], hash: u64) -> Option<TreeResource> {
    let weight: u64 = resources
        .iter()
        .map(|resource| u64::from(resource.weight))
        .sum();
    if weight == 0 {
        return None;
    }
    let mut choice = hash % weight;
    resources.iter().find_map(|resource| {
        if choice < u64::from(resource.weight) {
            return Some(*resource);
        }
        choice -= u64::from(resource.weight);
        None
    })
}

/// A preparation-worker result, published together with its tree generation.
/// Context coordinates use the same local Z-up frame as the tree geometry.
pub struct DecorationPlanner<'a, F: Fn([i16; 3]) -> SurroundingCell> {
    tree: BTreeMap<[i16; 3], TreeCell>,
    context: &'a F,
    output: BTreeMap<[i16; 3], Decoration>,
    reserved_air: BTreeSet<[i16; 3]>,
    attachment_supports: BTreeSet<[i16; 3]>,
    scope: DecoratorScope,
}

impl<'a, F: Fn([i16; 3]) -> SurroundingCell> DecorationPlanner<'a, F> {
    pub fn new(geometry: &TreeGeometry, context: &'a F) -> Self {
        Self {
            tree: geometry.cells().collect(),
            context,
            output: BTreeMap::new(),
            reserved_air: BTreeSet::new(),
            attachment_supports: BTreeSet::new(),
            scope: DecoratorScope::Tree,
        }
    }

    pub fn place(&mut self, mut state: TreeVoxelState, operation: DecorationOperation) -> bool {
        if self.output.len() >= 4096
            || state.position.iter().any(|value| value.unsigned_abs() > 52)
            || self.output.contains_key(&state.position)
            || self.reserved_air.contains(&state.position)
        {
            return false;
        }
        state.properties.sort_unstable();
        if state
            .properties
            .windows(2)
            .any(|pair| pair[0].0 == pair[1].0)
        {
            return false;
        }
        let tree_cell = self.tree.get(&state.position);
        let allowed = match operation {
            DecorationOperation::PlaceInAir => {
                tree_cell.is_none() && (self.context)(state.position) == SurroundingCell::Air
            }
            DecorationOperation::ReplaceSoil => {
                tree_cell.is_none()
                    && (self.context)(state.position) == SurroundingCell::ReplaceableSoil
            }
            DecorationOperation::ReplaceLog => {
                matches!(tree_cell, Some(TreeCell::Log(_)))
                    && !self.attachment_supports.contains(&state.position)
            }
        };
        if !allowed {
            return false;
        }
        self.output
            .insert(state.position, Decoration { state, operation });
        true
    }

    pub fn finish(self) -> Vec<Decoration> {
        self.output.into_values().collect()
    }

    /// Deterministic geometry-relative attachments. A nest samples probability
    /// once per tree; vines/cocoa sample each eligible support face. These are
    /// authored placement rules, not a reproduction of Minecraft's decorator.
    pub fn attach(&mut self, kind: Attachment, probability: f64, seed: u64) -> usize {
        let start = self.output.len();
        let salt = match kind {
            Attachment::BeeNest => 0xbee0,
            Attachment::TrunkVine => 0x7100,
            Attachment::LeafVine => 0x1ea0,
            Attachment::Cocoa => 0xc0c0,
            Attachment::ShelfMushroom => 0x5e1f,
        };
        if kind == Attachment::BeeNest && !selected(entropy(seed, [0; 3], salt), probability) {
            return 0;
        }
        // Descending height favours nests under the canopy rather than on roots.
        // Stable ordering also makes results independent of traversal/frame order.
        let mut supports: Vec<_> = self.tree.iter().map(|(p, c)| (*p, *c)).collect();
        supports.sort_unstable_by_key(|(p, _)| (-p[2], p[0], p[1]));
        for (position, cell) in supports {
            // The original tree snapshot is immutable. Pending replacements
            // must still affect subsequent attachment support decisions.
            if self.output.contains_key(&position) {
                continue;
            }
            if !self.matches_scope(cell) {
                continue;
            }
            let eligible = match kind {
                Attachment::LeafVine => cell == TreeCell::Leaf,
                Attachment::ShelfMushroom => matches!(cell, TreeCell::Log(_)),
                _ => matches!(cell, TreeCell::Log(_)) && position[2] >= 1,
            };
            if !eligible {
                continue;
            }
            let offset = (entropy(seed, position, salt) % 4) as usize;
            for index in 0..4 {
                let (delta, outward, inward) = SIDES[(index + offset) % 4];
                let target = [position[0] + delta[0], position[1] + delta[1], position[2]];
                if kind != Attachment::BeeNest
                    && !selected(entropy(seed, target, salt), probability)
                {
                    continue;
                }
                let (resource_id, properties) = match kind {
                    Attachment::BeeNest => {
                        let exit = [target[0] + delta[0], target[1] + delta[1], target[2]];
                        if !self.is_air(exit) {
                            continue;
                        }
                        (
                            "minecraft:bee_nest",
                            vec![("facing", outward), ("honey_level", "0")],
                        )
                    }
                    Attachment::Cocoa => {
                        let age = match entropy(seed, target, 0xa6e) % 3 {
                            0 => "0",
                            1 => "1",
                            _ => "2",
                        };
                        ("minecraft:cocoa", vec![("age", age), ("facing", inward)])
                    }
                    Attachment::ShelfMushroom => {
                        // The unrotated north-facing model reaches the south
                        // cell boundary (z16), so facing points away from wood.
                        let age = if entropy(seed, target, 0xa6e) & 1 == 0 {
                            "0"
                        } else {
                            "1"
                        };
                        (
                            "minecraft:shelf_mushroom",
                            vec![("age", age), ("facing", outward)],
                        )
                    }
                    Attachment::TrunkVine | Attachment::LeafVine => {
                        let mut properties = vec![
                            ("east", "false"),
                            ("north", "false"),
                            ("south", "false"),
                            ("up", "false"),
                            ("west", "false"),
                        ];
                        for (key, value) in &mut properties {
                            if *key == inward {
                                *value = "true";
                            }
                        }
                        ("minecraft:vine", properties)
                    }
                };
                let placed = self.place(
                    TreeVoxelState {
                        position: target,
                        resource_id,
                        properties,
                    },
                    DecorationOperation::PlaceInAir,
                );
                if placed {
                    self.attachment_supports.insert(position);
                }
                if placed && kind == Attachment::BeeNest {
                    // Preserve the exit against later decorations in this same
                    // preparation result; checking the initial snapshot alone
                    // would let later vines or litter close the entrance.
                    self.reserved_air.insert([
                        target[0] + delta[0],
                        target[1] + delta[1],
                        target[2],
                    ]);
                    return 1;
                }
            }
        }
        self.output.len() - start
    }

    fn is_air(&self, position: [i16; 3]) -> bool {
        position.iter().all(|value| value.unsigned_abs() <= 52)
            && !self.tree.contains_key(&position)
            && !self.output.contains_key(&position)
            && (self.context)(position) == SurroundingCell::Air
    }

    fn matches_scope(&self, cell: TreeCell) -> bool {
        match self.scope {
            DecoratorScope::Tree => true,
            DecoratorScope::FallenLog => {
                matches!(cell, TreeCell::Log(LogAxis::X | LogAxis::GroundY))
            }
            DecoratorScope::FallenStump => cell == TreeCell::Log(LogAxis::Vertical),
        }
    }

    pub fn log_mushrooms(
        &mut self,
        resources: &[TreeResource],
        probability: f64,
        seed: u64,
    ) -> usize {
        let start = self.output.len();
        let supports: Vec<_> = self.tree.iter().map(|(p, c)| (*p, *c)).collect();
        for (position, cell) in supports {
            if !matches!(cell, TreeCell::Log(_))
                || !self.matches_scope(cell)
                || self.output.contains_key(&position)
                || !selected(entropy(seed, position, 0xf061), probability)
            {
                continue;
            }
            let Some(resource) = choose_resource(resources, entropy(seed, position, 0x5a00)) else {
                continue;
            };
            let target = [position[0], position[1], position[2] + 1];
            if self.place(
                TreeVoxelState {
                    position: target,
                    resource_id: resource.id,
                    properties: resource.properties.to_vec(),
                },
                DecorationOperation::PlaceInAir,
            ) {
                self.attachment_supports.insert(position);
            }
        }
        self.output.len() - start
    }

    pub fn propagules(&mut self, recipe: DecoratorKind, seed: u64) -> usize {
        let DecoratorKind::Propagule {
            probability,
            minimum_age,
            maximum_age,
            exclusion_horizontal,
            exclusion_vertical,
            clearance,
        } = recipe
        else {
            return 0;
        };
        if minimum_age > maximum_age
            || maximum_age > 4
            || clearance == 0
            || clearance > 8
            || exclusion_horizontal > 12
            || exclusion_vertical > 12
        {
            return 0;
        }
        let start = self.output.len();
        let supports: Vec<_> = self
            .tree
            .iter()
            .filter_map(|(p, c)| (*c == TreeCell::Leaf).then_some(*p))
            .collect();
        let mut accepted: Vec<[i16; 3]> = Vec::new();
        for position in supports {
            if self.output.contains_key(&position)
                || !selected(entropy(seed, position, 0xb0d0), probability)
            {
                continue;
            }
            let target = [position[0], position[1], position[2] - 1];
            if accepted.iter().any(|p| {
                p[0].abs_diff(target[0]) <= u16::from(exclusion_horizontal)
                    && p[1].abs_diff(target[1]) <= u16::from(exclusion_horizontal)
                    && p[2].abs_diff(target[2]) <= u16::from(exclusion_vertical)
            }) || !(1..=clearance).all(|depth| {
                self.is_air([position[0], position[1], position[2] - i16::from(depth)])
            }) {
                continue;
            }
            let age = minimum_age
                + (entropy(seed, target, 0xa6e) % u64::from(maximum_age - minimum_age + 1)) as u8;
            let age = ["0", "1", "2", "3", "4"][usize::from(age)];
            if self.place(
                TreeVoxelState {
                    position: target,
                    resource_id: "minecraft:mangrove_propagule",
                    properties: vec![
                        ("age", age),
                        ("hanging", "true"),
                        ("stage", "0"),
                        ("waterlogged", "false"),
                    ],
                },
                DecorationOperation::PlaceInAir,
            ) {
                accepted.push(target);
                self.attachment_supports.insert(position);
                for depth in 2..=clearance {
                    self.reserved_air.insert([
                        position[0],
                        position[1],
                        position[2] - i16::from(depth),
                    ]);
                }
            }
        }
        self.output.len() - start
    }

    /// Original compact ground patches around trunk feet. ReplaceableSoil must
    /// mean membership in the supplied audited tag, classified by the caller.
    pub fn podzol(
        &mut self,
        replaceable_tag: &str,
        resource: TreeResource,
        ground_height: impl Fn([i16; 2]) -> Option<i16>,
    ) -> usize {
        if replaceable_tag != "minecraft:beneath_tree_podzol_replaceable" {
            return 0;
        }
        let start = self.output.len();
        let feet: BTreeSet<_> = self
            .tree
            .iter()
            .filter_map(|(p, c)| {
                (matches!(c, TreeCell::Log(LogAxis::Vertical)) && p[2] == 0).then_some([p[0], p[1]])
            })
            .collect();
        for foot in feet {
            for x in -3i16..=3 {
                for y in -3i16..=3 {
                    if x * x + y * y > 10 {
                        continue;
                    }
                    let position = [foot[0] + x, foot[1] + y];
                    let Some(height) = ground_height(position) else {
                        continue;
                    };
                    self.place(
                        TreeVoxelState {
                            position: [position[0], position[1], height],
                            resource_id: resource.id,
                            properties: resource.properties.to_vec(),
                        },
                        DecorationOperation::ReplaceSoil,
                    );
                }
            }
        }
        self.output.len() - start
    }

    /// One static dormant heart per selected tree, enclosed along its log axis.
    /// Candidate ordering and dormant display state are authored choices.
    pub fn heart(&mut self, probability: f64, seed: u64) -> usize {
        if !selected(entropy(seed, [0; 3], 0xceea), probability) {
            return 0;
        }
        let mut candidates: Vec<_> = self
            .tree
            .iter()
            .filter_map(|(p, c)| match c {
                TreeCell::Log(axis) if self.matches_scope(*c) => Some((*p, *axis)),
                _ => None,
            })
            .collect();
        candidates.sort_unstable_by_key(|(p, _)| entropy(seed, *p, 0xceeb));
        for (position, axis) in candidates {
            let (delta, property) = match axis {
                LogAxis::X => ([1, 0, 0], "x"),
                LogAxis::GroundY => ([0, 1, 0], "z"),
                LogAxis::Vertical => ([0, 0, 1], "y"),
            };
            let neighbours = [-1i16, 1].map(|sign| {
                [
                    position[0] + sign * delta[0],
                    position[1] + sign * delta[1],
                    position[2] + sign * delta[2],
                ]
            });
            if !neighbours.iter().all(|p| {
                self.tree.get(p) == Some(&TreeCell::Log(axis)) && !self.output.contains_key(p)
            }) {
                continue;
            }
            if self.place(
                TreeVoxelState {
                    position,
                    resource_id: "minecraft:creaking_heart",
                    properties: vec![("axis", property), ("creaking_heart_state", "dormant")],
                },
                DecorationOperation::ReplaceLog,
            ) {
                self.attachment_supports.extend(neighbours);
                return 1;
            }
        }
        0
    }

    /// Authored moss carpets under the crown and short chains on exposed
    /// undersides. Every chain is admitted together, preserving its sole tip.
    pub fn pale_moss(
        &mut self,
        ground: f64,
        leaves: f64,
        trunk: f64,
        seed: u64,
        ground_height: impl Fn([i16; 2]) -> Option<i16>,
    ) -> usize {
        let start = self.output.len();
        let supports: Vec<_> = self.tree.iter().map(|(p, c)| (*p, *c)).collect();
        for (position, cell) in supports {
            if self.output.contains_key(&position) {
                continue;
            }
            let probability = match cell {
                TreeCell::Leaf => leaves,
                TreeCell::Log(_) => trunk,
                _ => continue,
            };
            if !selected(entropy(seed, position, 0xa105), probability) {
                continue;
            }
            let mut chain = Vec::new();
            for depth in 1..=3 {
                let target = [position[0], position[1], position[2] - depth];
                if !self.is_air(target) || self.reserved_air.contains(&target) {
                    break;
                }
                chain.push(target);
            }
            if chain.is_empty() || self.output.len() + chain.len() > 4096 {
                continue;
            }
            let length = chain.len();
            for (index, target) in chain.into_iter().enumerate() {
                self.place(
                    TreeVoxelState {
                        position: target,
                        resource_id: "minecraft:pale_hanging_moss",
                        properties: vec![(
                            "tip",
                            if index + 1 == length { "true" } else { "false" },
                        )],
                    },
                    DecorationOperation::PlaceInAir,
                );
            }
            self.attachment_supports.insert(position);
        }
        for x in -6i16..=6 {
            for y in -6i16..=6 {
                if !self.tree.keys().any(|p| p[0] == x && p[1] == y) {
                    continue;
                }
                let Some(z) = ground_height([x, y]) else {
                    continue;
                };
                if !(-52..52).contains(&z)
                    || !matches!(
                        (self.context)([x, y, z]),
                        SurroundingCell::Solid | SurroundingCell::ReplaceableSoil
                    )
                    || !selected(entropy(seed, [x, y, z], 0xa106), ground)
                {
                    continue;
                }
                self.place(
                    TreeVoxelState {
                        position: [x, y, z + 1],
                        resource_id: "minecraft:pale_moss_carpet",
                        properties: vec![
                            ("bottom", "true"),
                            ("east", "none"),
                            ("north", "none"),
                            ("south", "none"),
                            ("west", "none"),
                        ],
                    },
                    DecorationOperation::PlaceInAir,
                );
            }
        }
        self.output.len() - start
    }

    /// Apply evidenced probabilities/providers in their retained source order.
    /// Authored placement parameters remain explicit in each coverage report;
    /// these methods do not reproduce the game's decorator algorithms.
    pub fn apply_profile(
        &mut self,
        profile: &DecorationProfile,
        seed: u64,
        ground_height: impl Fn([i16; 2]) -> Option<i16>,
    ) -> Vec<DecorationApplication> {
        let previous_scope = self.scope;
        let mut applications = Vec::with_capacity(profile.prescriptions.len());
        for (index, prescription) in profile.prescriptions.iter().enumerate() {
            self.scope = prescription.scope;
            let seed = entropy(seed, [index as i16, 0, 0], 0xdec0);
            let (cells, authored_parameters) = match prescription.kind {
                DecoratorKind::BeeNest(probability) => (
                    self.attach(Attachment::BeeNest, probability, seed),
                    &["natural bee_nest selection and support/entrance placement"][..],
                ),
                DecoratorKind::Cocoa(probability) => (
                    self.attach(Attachment::Cocoa, probability, seed),
                    &["per-face sampling and deterministic visual growth age"][..],
                ),
                DecoratorKind::ShelfMushroom(probability) => (
                    self.attach(Attachment::ShelfMushroom, probability, seed),
                    &["per-face sampling and deterministic visual age0/1"][..],
                ),
                DecoratorKind::LeafVine(probability) => (
                    self.attach(Attachment::LeafVine, probability, seed),
                    &["geometry-relative support-face sampling"][..],
                ),
                DecoratorKind::TrunkVine => (
                    self.attach(Attachment::TrunkVine, 0.3, seed),
                    &["homage probability0.3; source config does not expose a probability"][..],
                ),
                DecoratorKind::Ground {
                    resources,
                    tries,
                    radius: Some(radius),
                    ..
                } => (
                    self.scatter_ground(resources, tries, radius, seed, &ground_height),
                    &["scatter distribution follows actual ground heights"][..],
                ),
                DecoratorKind::Ground { resources, tries, radius: None, .. } => (
                    self.scatter_ground(resources, tries, 6, seed, &ground_height),
                    &["authored radius6 for source-omitted radius; terrain-height scatter replaces codec vertical search"][..],
                ),
                DecoratorKind::LogMushroom {
                    resources,
                    probability,
                } => (
                    self.log_mushrooms(resources, probability, seed),
                    &["probability sampled per exposed source-scope log top"][..],
                ),
                recipe @ DecoratorKind::Propagule { .. } => (
                    self.propagules(recipe, seed),
                    &["deterministic geometry-relative exclusion order"][..],
                ),
                DecoratorKind::Podzol { replaceable_tag, resource } => (
                    self.podzol(replaceable_tag, resource, &ground_height),
                    &["radius3 compact trunk-foot patches; caller classifies exact replaceable tag"][..],
                ),
                DecoratorKind::Heart(probability) => (
                    self.heart(probability, seed),
                    &["one axis-enclosed heart, static dormant display and candidate order"][..],
                ),
                DecoratorKind::PaleMoss { ground, leaves, trunk } => (
                    self.pale_moss(ground, leaves, trunk, seed, &ground_height),
                    &["chains at most3cells on exposed undersides; radius6 crown-projected ground carpets"][..],
                ),
            };
            applications.push(DecorationApplication {
                source_kind: prescription.source_kind,
                scope: prescription.scope,
                status: ApplicationStatus::Applied {
                    cells,
                    authored_parameters,
                },
            });
        }
        self.scope = previous_scope;
        applications
    }

    /// Scatter onto the caller's real terrain heights, not a flat plane at the
    /// tree origin. The height callback and occupancy snapshot must belong to
    /// the same generation. Source provider properties and weights are retained.
    pub fn scatter_ground(
        &mut self,
        resources: &[TreeResource],
        tries: u16,
        radius: u8,
        seed: u64,
        ground_height: impl Fn([i16; 2]) -> Option<i16>,
    ) -> usize {
        let total_weight: u64 = resources
            .iter()
            .map(|resource| u64::from(resource.weight))
            .sum();
        if total_weight == 0 || tries > 512 || radius > 12 {
            return 0;
        }
        let start = self.output.len();
        let width = u64::from(radius) * 2 + 1;
        for attempt in 0..tries {
            let hash = entropy(seed, [attempt as i16, 0, 0], 0x0011_77e2);
            let x = (hash % width) as i16 - i16::from(radius);
            let y = ((hash / width) % width) as i16 - i16::from(radius);
            let Some(z) = ground_height([x, y]) else {
                continue;
            };
            if !(-52..52).contains(&z)
                || !matches!(
                    (self.context)([x, y, z]),
                    SurroundingCell::Solid | SurroundingCell::ReplaceableSoil
                )
            {
                continue;
            }
            let mut choice = entropy(seed, [x, y, z], 0x1eaf) % total_weight;
            let Some(resource) = resources.iter().find(|resource| {
                if choice < u64::from(resource.weight) {
                    return true;
                }
                choice -= u64::from(resource.weight);
                false
            }) else {
                continue;
            };
            self.place(
                TreeVoxelState {
                    position: [x, y, z + 1],
                    resource_id: resource.id,
                    properties: resource.properties.to_vec(),
                },
                DecorationOperation::PlaceInAir,
            );
        }
        self.output.len() - start
    }
}

#[cfg(test)]
mod tests {
    use super::super::tree_geometry::LogAxis;
    use super::*;

    fn state(position: [i16; 3], resource_id: &'static str) -> TreeVoxelState {
        TreeVoxelState {
            position,
            resource_id,
            properties: vec![],
        }
    }

    #[test]
    fn preserves_tree_and_foreign_occupancy_but_accepts_known_air() {
        let mut tree = TreeGeometry::default();
        tree.branch([0, 0, 0], [0, 0, 3]).unwrap();
        let context = |p: [i16; 3]| match p[0] {
            1 => SurroundingCell::Solid,
            2 => SurroundingCell::Unknown,
            _ => SurroundingCell::Air,
        };
        let mut planner = DecorationPlanner::new(&tree, &context);
        for p in [[0, 0, 1], [1, 0, 1], [2, 0, 1]] {
            assert!(!planner.place(
                state(p, "minecraft:bee_nest"),
                DecorationOperation::PlaceInAir
            ));
        }
        assert!(planner.place(
            state([-1, 0, 1], "minecraft:bee_nest"),
            DecorationOperation::PlaceInAir
        ));
        assert_eq!(planner.finish().len(), 1);
    }

    #[test]
    fn replacement_operations_only_accept_their_declared_substrate() {
        let mut tree = TreeGeometry::default();
        tree.branch([0, 0, 0], [0, 0, 3]).unwrap();
        tree.canopy([2, 0, 3], [1, 1, 1]).unwrap();
        let context = |p: [i16; 3]| {
            if p[2] == -1 {
                SurroundingCell::ReplaceableSoil
            } else {
                SurroundingCell::Air
            }
        };
        let mut planner = DecorationPlanner::new(&tree, &context);
        assert!(planner.place(
            state([0, 0, 1], "minecraft:creaking_heart"),
            DecorationOperation::ReplaceLog
        ));
        assert!(!planner.place(
            state([2, 0, 3], "minecraft:creaking_heart"),
            DecorationOperation::ReplaceLog
        ));
        assert!(planner.place(
            state([0, 0, -1], "minecraft:podzol"),
            DecorationOperation::ReplaceSoil
        ));
        assert!(!planner.place(
            state([0, 0, -2], "minecraft:podzol"),
            DecorationOperation::ReplaceSoil
        ));
        let cells = planner.finish();
        assert_eq!(cells.len(), 2);
        assert_eq!(
            tree.cells().find(|(p, _)| *p == [0, 0, 1]).unwrap().1,
            TreeCell::Log(LogAxis::Vertical)
        );
    }

    #[test]
    fn collision_is_first_owner_and_property_order_is_canonical() {
        let tree = TreeGeometry::default();
        let context = |_| SurroundingCell::Air;
        let mut planner = DecorationPlanner::new(&tree, &context);
        let mut vine = state([0, 0, 1], "minecraft:vine");
        vine.properties = vec![("west", "false"), ("east", "true")];
        assert!(planner.place(vine, DecorationOperation::PlaceInAir));
        assert!(!planner.place(
            state([0, 0, 1], "minecraft:cocoa"),
            DecorationOperation::PlaceInAir
        ));
        assert_eq!(
            planner.finish()[0].state.properties,
            vec![("east", "true"), ("west", "false")]
        );
    }

    #[test]
    fn rejects_out_of_bounds_and_duplicate_properties_without_partial_publication() {
        let tree = TreeGeometry::default();
        let context = |_| SurroundingCell::Air;
        let mut planner = DecorationPlanner::new(&tree, &context);
        assert!(!planner.place(
            state([53, 0, 0], "minecraft:vine"),
            DecorationOperation::PlaceInAir
        ));
        let mut invalid = state([0, 0, 0], "minecraft:vine");
        invalid.properties = vec![("east", "true"), ("east", "false")];
        assert!(!planner.place(invalid, DecorationOperation::PlaceInAir));
        assert!(planner.finish().is_empty());
    }

    #[test]
    fn nest_has_log_support_and_an_unobstructed_outward_exit() {
        let mut tree = TreeGeometry::default();
        tree.branch([0, 0, 0], [0, 0, 4]).unwrap();
        let context = |p: [i16; 3]| {
            if p[0] < 0 || p[1] != 0 {
                SurroundingCell::Solid
            } else {
                SurroundingCell::Air
            }
        };
        let mut planner = DecorationPlanner::new(&tree, &context);
        assert_eq!(planner.attach(Attachment::BeeNest, 1.0, 17), 1);
        assert!(!planner.place(
            state([2, 0, 4], "minecraft:cocoa"),
            DecorationOperation::PlaceInAir
        ));
        let nest = &planner.finish()[0].state;
        assert_eq!(nest.position, [1, 0, 4]);
        assert_eq!(
            nest.properties,
            vec![("facing", "east"), ("honey_level", "0")]
        );
        let blocked = |_| SurroundingCell::Solid;
        let mut planner = DecorationPlanner::new(&tree, &blocked);
        assert_eq!(planner.attach(Attachment::BeeNest, 1.0, 17), 0);
    }

    #[test]
    fn attachment_probabilities_and_context_keep_results_stable_and_bounded() {
        let mut tree = TreeGeometry::default();
        tree.branch([0, 0, 0], [0, 0, 4]).unwrap();
        let context = |_| SurroundingCell::Air;
        for probability in [0.0, -1.0, 2.0, f64::NAN, f64::INFINITY] {
            let mut planner = DecorationPlanner::new(&tree, &context);
            assert_eq!(planner.attach(Attachment::BeeNest, probability, 17), 0);
            assert_eq!(planner.attach(Attachment::TrunkVine, probability, 17), 0);
        }
        let build = || {
            let mut planner = DecorationPlanner::new(&tree, &context);
            assert_eq!(planner.attach(Attachment::Cocoa, 1.0, 17), 16);
            planner.finish()
        };
        assert_eq!(build(), build());
        for decoration in build() {
            let state = decoration.state;
            let facing = state
                .properties
                .iter()
                .find(|(key, _)| *key == "facing")
                .unwrap()
                .1;
            let expected = match (state.position[0], state.position[1]) {
                (1, 0) => "west",
                (-1, 0) => "east",
                (0, 1) => "north",
                _ => "south",
            };
            assert_eq!(facing, expected);
        }
    }

    #[test]
    fn litter_follows_sloped_terrain_and_keeps_provider_properties() {
        let tree = TreeGeometry::default();
        let context = |p: [i16; 3]| {
            if p[2] < p[0] {
                SurroundingCell::ReplaceableSoil
            } else {
                SurroundingCell::Air
            }
        };
        let resources = [TreeResource {
            id: "minecraft:leaf_litter",
            properties: &[("facing", "south"), ("segment_amount", "3")],
            weight: 1,
        }];
        let mut planner = DecorationPlanner::new(&tree, &context);
        assert!(planner.scatter_ground(&resources, 96, 4, 77, |p| Some(p[0] - 1)) > 20);
        for decoration in planner.finish() {
            let state = decoration.state;
            assert_eq!(state.position[2], state.position[0]);
            assert_eq!(state.properties, resources[0].properties);
        }
        let mut planner = DecorationPlanner::new(&tree, &context);
        assert_eq!(planner.scatter_ground(&resources, 96, 4, 77, |_| None), 0);
        assert_eq!(
            planner.scatter_ground(&resources, 96, 4, 77, |_| Some(52)),
            0
        );
        assert_eq!(
            planner.scatter_ground(&resources, 513, 4, 77, |_| Some(-1)),
            0
        );
    }

    #[test]
    fn replacements_and_attachments_preserve_final_support_in_both_orders() {
        let mut tree = TreeGeometry::default();
        tree.branch([0, 0, 0], [0, 0, 3]).unwrap();
        let context = |_| SurroundingCell::Air;
        let mut planner = DecorationPlanner::new(&tree, &context);
        for z in 0..=3 {
            assert!(planner.place(
                state([0, 0, z], "minecraft:creaking_heart"),
                DecorationOperation::ReplaceLog
            ));
        }
        assert_eq!(planner.attach(Attachment::Cocoa, 1.0, 17), 0);
        let mut planner = DecorationPlanner::new(&tree, &context);
        assert_eq!(planner.attach(Attachment::Cocoa, 1.0, 17), 12);
        for z in 1..=3 {
            assert!(!planner.place(
                state([0, 0, z], "minecraft:creaking_heart"),
                DecorationOperation::ReplaceLog
            ));
        }
    }

    #[test]
    fn source_binding_uses_guaranteed_nest_profile_and_reports_pending_families() {
        use super::super::{tree_decoration_profiles, tree_forms, tree_profiles};
        let profile = tree_profiles::profile("minecraft:fancy_oak_bees").unwrap();
        let geometry = tree_forms::build(profile, tree_forms::Growth::Mature, 77).unwrap();
        let context = |p: [i16; 3]| {
            if p[2] < 0 {
                SurroundingCell::ReplaceableSoil
            } else {
                SurroundingCell::Air
            }
        };
        let mut planner = DecorationPlanner::new(&geometry, &context);
        let report = planner.apply_profile(
            tree_decoration_profiles::profile(profile.id).unwrap(),
            77,
            |_| Some(-1),
        );
        assert!(report
            .iter()
            .any(|row| row.source_kind == "minecraft:beehive"
                && matches!(row.status, ApplicationStatus::Applied { cells: 1, .. })));
        let ground_profile =
            tree_decoration_profiles::profile("minecraft:birch_bees_0002_leaf_litter").unwrap();
        assert!(ground_profile
            .prescriptions
            .iter()
            .any(|row| matches!(row.kind, DecoratorKind::Ground { radius: None, .. })));
        let ground_report = planner.apply_profile(ground_profile, 77, |_| Some(-1));
        assert!(ground_report
            .iter()
            .all(|row| matches!(row.status, ApplicationStatus::Applied { .. })));
        assert!(planner
            .finish()
            .iter()
            .any(|row| row.state.resource_id == "minecraft:bee_nest"));
        for profile in tree_profiles::TREE_PROFILES {
            let prescriptions = tree_decoration_profiles::profile(profile.id).unwrap();
            assert_eq!(prescriptions.source_sha256, profile.source_sha256);
        }
    }

    #[test]
    fn propagules_keep_source_age_clearance_and_horizontal_exclusion() {
        let mut geometry = TreeGeometry::default();
        geometry.canopy([0, 0, 5], [4, 4, 0]).unwrap();
        let context = |p: [i16; 3]| {
            if p[0] == 0 && p[2] == 3 {
                SurroundingCell::Solid
            } else {
                SurroundingCell::Air
            }
        };
        let mut planner = DecorationPlanner::new(&geometry, &context);
        let recipe = DecoratorKind::Propagule {
            probability: 1.0,
            minimum_age: 0,
            maximum_age: 4,
            exclusion_horizontal: 1,
            exclusion_vertical: 0,
            clearance: 2,
        };
        assert!(planner.propagules(recipe, 77) > 5);
        let output = planner.finish();
        for (index, row) in output.iter().enumerate() {
            assert_eq!(row.state.position[2], 4);
            assert_ne!(row.state.position[0], 0);
            assert_eq!(
                row.state
                    .properties
                    .iter()
                    .find(|(k, _)| *k == "hanging")
                    .unwrap()
                    .1,
                "true"
            );
            for other in &output[index + 1..] {
                assert!(
                    row.state.position[0].abs_diff(other.state.position[0]) > 1
                        || row.state.position[1].abs_diff(other.state.position[1]) > 1
                );
            }
        }
    }

    #[test]
    fn fallen_log_mushrooms_do_not_borrow_upright_stump_scope() {
        let mut geometry = TreeGeometry::default();
        geometry.branch([0, 0, 0], [0, 0, 2]).unwrap();
        geometry.branch([3, 0, 0], [7, 0, 0]).unwrap();
        let context = |_| SurroundingCell::Air;
        let mut planner = DecorationPlanner::new(&geometry, &context);
        planner.scope = DecoratorScope::FallenLog;
        let resources = [TreeResource {
            id: "minecraft:brown_mushroom",
            properties: &[],
            weight: 1,
        }];
        assert_eq!(planner.log_mushrooms(&resources, 1.0, 77), 5);
        assert!(planner
            .finish()
            .iter()
            .all(|row| row.state.position[0] >= 3));
    }

    #[test]
    fn podzol_requires_tag_qualified_soil_at_actual_ground_height() {
        let mut geometry = TreeGeometry::default();
        geometry.branch([0, 0, 0], [0, 0, 5]).unwrap();
        let context = |p: [i16; 3]| {
            if p[2] != -2 {
                SurroundingCell::Air
            } else if p[0] < 0 {
                SurroundingCell::ReplaceableSoil
            } else {
                SurroundingCell::Solid
            }
        };
        let mut planner = DecorationPlanner::new(&geometry, &context);
        let resource = TreeResource {
            id: "minecraft:podzol",
            properties: &[("snowy", "false")],
            weight: 1,
        };
        assert_eq!(
            planner.podzol("minecraft:unrelated", resource, |_| Some(-2)),
            0
        );
        assert!(
            planner.podzol(
                "minecraft:beneath_tree_podzol_replaceable",
                resource,
                |_| Some(-2)
            ) > 0
        );
        assert!(planner.finish().iter().all(|row| row.state.position[0] < 0
            && row.state.position[2] == -2
            && row.operation == DecorationOperation::ReplaceSoil));
    }

    #[test]
    fn heart_preserves_log_axis_neighbours_and_attachment_support() {
        let mut geometry = TreeGeometry::default();
        geometry.branch([0, 0, 0], [0, 0, 5]).unwrap();
        let context = |_| SurroundingCell::Air;
        let mut planner = DecorationPlanner::new(&geometry, &context);
        planner
            .attachment_supports
            .extend([[0, 0, 1], [0, 0, 2], [0, 0, 3]]);
        assert_eq!(planner.heart(0.0, 99), 0);
        assert_eq!(planner.heart(1.0, 99), 1);
        let output = planner.finish();
        assert_eq!(output[0].state.position, [0, 0, 4]);
        assert_eq!(
            output[0].state.properties,
            vec![("axis", "y"), ("creaking_heart_state", "dormant")]
        );
        assert_eq!(output[0].operation, DecorationOperation::ReplaceLog);
    }

    #[test]
    fn shelf_mushrooms_face_away_from_the_retained_log_support() {
        let mut geometry = TreeGeometry::default();
        geometry.branch([0, 0, 0], [0, 0, 3]).unwrap();
        let context = |_| SurroundingCell::Air;
        let mut planner = DecorationPlanner::new(&geometry, &context);
        assert_eq!(planner.attach(Attachment::ShelfMushroom, 0.0, 37), 0);
        assert_eq!(planner.attach(Attachment::ShelfMushroom, 1.0, 37), 16);
        for row in planner.finish() {
            let p = row.state.position;
            let expected = match [p[0], p[1]] {
                [1, 0] => "east",
                [-1, 0] => "west",
                [0, 1] => "south",
                [0, -1] => "north",
                _ => panic!("unsupported shelf"),
            };
            assert!(row.state.properties.contains(&("facing", expected)));
            assert!(row
                .state
                .properties
                .iter()
                .any(|(k, v)| *k == "age" && ["0", "1"].contains(v)));
        }
    }

    #[test]
    fn pale_moss_chains_stop_before_obstacles_and_only_the_last_cell_is_a_tip() {
        let mut geometry = TreeGeometry::default();
        geometry.canopy([3, 0, 6], [0, 0, 0]).unwrap();
        let context = |p: [i16; 3]| {
            if p[2] <= 3 {
                SurroundingCell::Solid
            } else {
                SurroundingCell::Air
            }
        };
        let mut planner = DecorationPlanner::new(&geometry, &context);
        assert_eq!(planner.pale_moss(0.0, 1.0, 0.0, 17, |_| None), 2);
        let rows = planner.finish();
        assert_eq!(rows[0].state.position, [3, 0, 4]);
        assert_eq!(rows[0].state.properties, vec![("tip", "true")]);
        assert_eq!(rows[1].state.position, [3, 0, 5]);
        assert_eq!(rows[1].state.properties, vec![("tip", "false")]);
        assert!(rows
            .iter()
            .all(|row| row.state.resource_id == "minecraft:pale_hanging_moss"));
    }
}
