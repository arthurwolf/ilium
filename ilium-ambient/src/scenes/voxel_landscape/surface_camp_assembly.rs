//! Complete camp terrain admission before projection. The global grid, sparse
//! selection and bounded foundation cuts are original Ilium placement choices.
use super::{
    assets::{
        block_state::BlockState,
        error::{AssetError, Result},
        identity::ResourceId,
    },
    noise::hash2,
    settings::VoxelLandscapeSettings,
    surface_biome_selector,
    surface_camps::{self, CampStyle},
    surface_ruins::SurfaceSample,
    surface_structures::{self, Habitat, PlacementError, Prepared, TemplateCell},
    terrain_fields::TerrainFields,
};
use std::collections::BTreeMap;

pub struct CampPlacement {
    pub style: CampStyle,
    pub prepared: Prepared<BlockState>,
}
fn state(id: &str, props: &[(&str, &str)]) -> std::result::Result<BlockState, PlacementError> {
    BlockState::new(
        ResourceId::parse(id).map_err(|_| PlacementError::InvalidState)?,
        props.iter().map(|(k, v)| (k.to_string(), v.to_string())),
    )
    .map_err(|_| PlacementError::InvalidState)
}
fn error(e: PlacementError) -> AssetError {
    if e == PlacementError::Cancelled {
        AssetError::Cancelled
    } else {
        AssetError::InvalidMetadata(format!("camp placement: {e:?}"))
    }
}
fn global_xy(anchor: [i32; 3], [x, y]: [i32; 2], turns: u8) -> Option<[i32; 2]> {
    let [x, y] = match turns % 4 {
        1 => [-y, x],
        2 => [-x, -y],
        3 => [y, -x],
        _ => [x, y],
    };
    Some([anchor[0].checked_add(x)?, anchor[1].checked_add(y)?])
}
// Fit only the fixed Windswept Forest clearing; ground coordinates are inclusive.
const WINDSWEPT_WORK: usize = 28 * 28 * 3; // Exclude the existing template floor.
fn windswept_dry(ground: &SurfaceSample) -> bool {
    // Validate before height arithmetic.
    (0..=256).contains(&ground.ground) // Require a representable supporting solid.
        && ground.water.is_none_or(|water| water <= ground.ground) // Reject wet sites.
} // End dry-ground admission.
fn assemble_windswept(
    // Keep all other camp styles on the original path.
    anchor: [i32; 3], // The public caller already checked the nominal floor bounds.
    turns: u8,        // Use this same rotation for terrain, supports, and authored states.
    seed: u64,        // Preserve the selected source layout and identity.
    sample: impl Fn([i32; 2]) -> Option<SurfaceSample>, // Immutable terrain snapshot.
    cancelled: &impl Fn() -> bool, // Never publish a partially prepared camp.
) -> Result<Option<CampPlacement>> {
    // Absence is distinct from cancellation/error.
    let mut columns = BTreeMap::<[i32; 2], SurfaceSample>::new(); // Full global footprint.
    let mut grounds = Vec::<i32>::with_capacity(784); // Bounded floor-cost inputs.
    let (mut lower, mut upper): (i32, i32) = (1, 249); // Keep support and seven air levels.
    for y in 0..28 {
        // Visit the entire local clearing, independent of the window.
        for x in 0..28 {
            // Each local column is sampled exactly once.
            if cancelled() {
                // Poll before each terrain callback.
                return Err(AssetError::Cancelled); // Discard all provisional state.
            } // End cancellation guard.
            let Some(xy) = global_xy(anchor, [x, y], turns) else {
                // Checked rotation/offset.
                return Ok(None); // Reject coordinate overflow atomically.
            }; // End coordinate guard.
            let Some(ground) = sample(xy).filter(windswept_dry) else {
                // Require known dry ground.
                return Ok(None); // Do not fill water or unknown terrain.
            }; // End terrain guard.
            for local in [[x - 1, y], [x, y - 1]] {
                // Compare both prior cardinal neighbors.
                if local[0] < 0 || local[1] < 0 {
                    // Ignore positions outside the footprint.
                    continue; // External access is checked separately below.
                } // End edge guard.
                let Some(neighbor) = global_xy(anchor, local, turns) else {
                    // Same rotation.
                    return Ok(None); // Fail closed on coordinate overflow.
                }; // End neighbor-coordinate guard.
                let Some(other) = columns.get(&neighbor) else {
                    // Prior samples must exist.
                    return Ok(None); // Never assume a missing neighbor is flat.
                }; // End neighbor-presence guard.
                if (ground.ground - other.ground).abs() > 2 {
                    // Preserve the local step cap.
                    return Ok(None); // Reject cliffs rather than burying them.
                } // End local-relief guard.
            } // End neighbor checks.
            lower = lower.max(ground.ground - 2); // At most two solids above the floor cut.
            upper = upper.min(ground.ground + 7); // At most six support blocks below the floor.
            if let Some(water) = ground.water {
                // A dry site's water datum still constrains cuts.
                lower = lower.max(water); // Never lower the floor below that datum.
            } // End water-floor constraint.
            grounds.push(ground.ground); // Retain the inclusive height for bounded cost evaluation.
            columns.insert(xy, ground); // Keep rotated keys for final habitat validation.
        } // End local X traversal.
    } // End local Y traversal.
    grounds.sort_unstable(); // Deterministic integer relief and cost inputs.
    if grounds[783] - grounds[0] > 8 {
        // Cap relief across the complete clearing.
        return Ok(None); // Reject a broad slope outside the selected envelope.
    } // End complete-relief guard.
    for x in 12..=14 {
        // The clear central lane meets the existing path at local [13,1].
        if cancelled() {
            // Approach sampling is also cancellable.
            return Err(AssetError::Cancelled); // No prepared camp escapes cancellation.
        } // End cancellation guard.
        let Some(xy) = global_xy(anchor, [x, -1], turns) else {
            // Rotate the external apron too.
            return Ok(None); // Reject an unrepresentable approach.
        }; // End approach-coordinate guard.
        let Some(ground) = sample(xy).filter(windswept_dry) else {
            // Read-only dry approaches.
            return Ok(None); // No external terrain is flattened or bridged.
        }; // End approach-terrain guard.
        lower = lower.max(ground.ground - 1); // At most one step down into the clearing.
        upper = upper.min(ground.ground + 1); // At most one step up into the clearing.
    } // End approach constraints; their intersection has at most three floors.
    if lower > upper {
        // All per-column and approach constraints must intersect.
        return Ok(None); // Do not choose an arbitrary out-of-policy elevation.
    } // End feasible-interval guard.
    let mut best = None::<(usize, u32, i32)>; // Work, nominal displacement, then lower floor.
    for floor in lower..=upper {
        // Evaluate at most three integer elevations.
        if cancelled() {
            // Bound cancellation latency during optimization.
            return Err(AssetError::Cancelled); // Discard provisional floor selection.
        } // End cancellation guard.
        let work: usize = grounds
            .iter()
            .map(|ground| {
                // Count only actual cut/fill additions.
                ((floor - ground - 1).max(0) + (ground - floor).max(0)) as usize
                // Floor excluded.
            })
            .sum(); // At most 784 bounded terms per candidate floor.
        let choice = (work, floor.abs_diff(anchor[2]), floor); // Deterministic tie order.
        if best.is_none_or(|previous| choice < previous) {
            // Select the least earthwork.
            best = Some(choice); // No horizontal relocation or alternative layout.
        } // End best-floor update.
    } // End bounded optimization.
    let Some((work, _, floor)) = best else {
        // Retain fail-closed absence semantics.
        return Ok(None); // A missing feasible floor never manufactures a camp.
    }; // End selected-floor guard.
    if work > WINDSWEPT_WORK {
        // Enforce the whole-footprint earthwork budget.
        return Ok(None); // Per-column caps alone do not authorize excessive fill.
    } // End budget guard.
    let style = CampStyle::WindsweptForest; // Only this style uses the new policy.
    let mut kit = surface_camps::build(style, seed, state).map_err(error)?; // Original kit.
    if kit.template.cells.len() != 28 * 28 * 8 {
        // Require the captured complete reservation.
        return Err(error(PlacementError::Bounds)); // Future kit changes need a reviewed fit.
    } // End template-size guard.
    for cell in &kit.template.cells {
        // Check the fixed source bounds before adding supports.
        if cancelled() {
            // The complete authored reservation remains cancellable.
            return Err(AssetError::Cancelled); // Nothing has been published.
        } // End cancellation guard.
        if !(0..28).contains(&cell.position[0]) // Keep the original X footprint.
            || !(0..28).contains(&cell.position[1]) // Keep the original Y footprint.
            || !(0..=7).contains(&cell.position[2]) // Keep seven explicit air/geometry levels.
            || (cell.position[2] == 0 && cell.state.is_none())
        // Every column needs its floor.
        {
            // Reject incompatible source geometry, not ordinary terrain absence.
            return Err(error(PlacementError::Bounds)); // Never patch over a missing floor.
        } // End source-geometry guard.
    } // End source validation; Prepared still checks duplicates and source identity.
    let support = state("minecraft:dirt", &[]).map_err(error)?; // Preserve this style's support soil.
    let anchor = [anchor[0], anchor[1], floor]; // Publish the actual fitted floor in the owner key.
    for y in 0..28 {
        // Add supports in source-local coordinates only.
        for x in 0..28 {
            // The final preparer rotates each support exactly once.
            if cancelled() {
                // Poll before each support column.
                return Err(AssetError::Cancelled); // Drop the complete provisional kit.
            } // End cancellation guard.
            let Some(xy) = global_xy(anchor, [x, y], turns) else {
                // Same global footprint.
                return Ok(None); // Fail closed even though earlier validation succeeded.
            }; // End support-coordinate guard.
            let ground = columns[&xy].ground; // Use the original sampled inclusive solid top.
            for z in ground + 1..floor {
                // Fill to, but never overwrite, the authored floor.
                kit.template.cells.push(TemplateCell {
                    // Preserve every existing solid and air cell.
                    position: [x, y, z - floor], // Negative local offsets are bounded by six.
                    state: Some(support.clone()), // Never carve below the fitted floor.
                }); // End support cell.
            } // End support height traversal.
        } // End support X traversal.
    } // End support Y traversal.
    let prepared = Prepared::prepare(
        // Reuse the unchanged atomic preparation boundary.
        &kit.template, // Contains every original air cell plus bounded solid supports.
        anchor,        // Keep the actual fitted floor in every later projection.
        turns,         // Preserve position/state rotation and source rotation identity.
        |block, rotation| {
            // Same state transform as the original camp path.
            BlockState::new(
                // Do not replace semantic state properties with defaults.
                block.id().clone(), // Keep the authored resource identifier.
                surface_structures::rotate_properties(block.properties(), rotation)?, // Exact rotation.
            )
            .map_err(|_| PlacementError::InvalidState) // Reject invalid transformed states.
        }, // End state adapter.
        |position| {
            // Check the complete global reservation, including explicit air.
            if !(0..=256).contains(&position[2]) // Bound supports and all seven air levels.
                || !columns.contains_key(&[position[0], position[1]])
            // No footprint expansion.
            {
                // Unknown positions reject the complete preparation.
                return Habitat::Unknown; // Do not reserve unsupported exterior space.
            } // End habitat guard.
            Habitat::Replaceable // Terrain and floor constraints were validated above.
        }, // End habitat adapter.
        cancelled,     // Prepared checks cancellation per cell and before publication.
    ); // End atomic preparation.
    match prepared { // Preserve the existing distinction between absence and errors.
        Ok(prepared) => Ok(Some(CampPlacement { style, prepared })), // One whole placement.
        Err(PlacementError::Unknown(_) | PlacementError::Protected(_) // Ordinary admission rejection.
            | PlacementError::CoordinateOverflow | PlacementError::Bounds) => Ok(None), // No fragment.
        Err(problem) => Err(error(problem)), // Includes cancellation, duplicates, and invalid states.
    } // End preparation disposition.
} // End Windswept Forest adaptation.
pub fn assemble(
    style: CampStyle,
    anchor: [i32; 3],
    turns: u8,
    seed: u64,
    sample: impl Fn([i32; 2]) -> Option<SurfaceSample>,
    cancelled: impl Fn() -> bool,
) -> Result<Option<CampPlacement>> {
    if cancelled() {
        return Err(AssetError::Cancelled);
    }
    if !(0..=249).contains(&anchor[2]) {
        return Ok(None);
    }
    if style == CampStyle::WindsweptForest {
        // Apply the bounded fit only to the missing family.
        return assemble_windswept(anchor, turns, seed, sample, &cancelled); // Preserve other styles.
    } // End targeted adaptation dispatch.
    let mut kit = surface_camps::build(style, seed, state).map_err(error)?;
    let mut columns = BTreeMap::new();
    // Each complete column is checked once. Unknown or wet terrain rejects the
    // entire camp; lower columns receive short supports rather than float.
    let support = state(
        if style == CampStyle::WoodedBadlands {
            "minecraft:red_sandstone"
        } else {
            "minecraft:dirt"
        },
        &[],
    )
    .map_err(error)?;
    for y in 0..28 {
        for x in 0..28 {
            if cancelled() {
                return Err(AssetError::Cancelled);
            }
            let Some(xy) = global_xy(anchor, [x, y], turns) else {
                return Ok(None);
            };
            let Some(ground) = sample(xy) else {
                return Ok(None);
            };
            if (i64::from(ground.ground) - i64::from(anchor[2])).abs() > 2
                || ground.water.is_some_and(|water| water > ground.ground)
            {
                return Ok(None);
            }
            columns.insert(xy, ground);
            for z in ground.ground + 1..anchor[2] {
                kit.template.cells.push(TemplateCell {
                    position: [x, y, z - anchor[2]],
                    state: Some(support.clone()),
                });
            }
        }
    }
    let prepared = Prepared::prepare(
        &kit.template,
        anchor,
        turns,
        |s, n| {
            BlockState::new(
                s.id().clone(),
                surface_structures::rotate_properties(s.properties(), n)?,
            )
            .map_err(|_| PlacementError::InvalidState)
        },
        |p| {
            if !(0..=256).contains(&p[2]) || !columns.contains_key(&[p[0], p[1]]) {
                Habitat::Unknown
            } else {
                Habitat::Replaceable
            }
        },
        &cancelled,
    );
    match prepared {
        Ok(prepared) => Ok(Some(CampPlacement { style, prepared })),
        Err(PlacementError::Cancelled) => Err(AssetError::Cancelled),
        Err(
            PlacementError::Unknown(_)
            | PlacementError::Protected(_)
            | PlacementError::CoordinateOverflow
            | PlacementError::Bounds,
        ) => Ok(None),
        Err(e) => Err(error(e)),
    }
}
pub fn candidate(
    grid: [i32; 2],
    fields: &TerrainFields,
    settings: &VoxelLandscapeSettings,
    cancelled: impl Fn() -> bool,
) -> Result<Option<CampPlacement>> {
    if cancelled() {
        return Err(AssetError::Cancelled);
    }
    let seed = u64::from(settings.seed);
    let hash = hash2(
        seed ^ 0x6361_6d70_3030_3031,
        i64::from(grid[0]),
        i64::from(grid[1]),
    );
    if hash % 100 >= settings.structures_percent.clamp(0, 200) as u64 / 8 {
        return Ok(None);
    }
    let Some(x) = grid[0]
        .checked_mul(128)
        .and_then(|v| v.checked_add(32 + ((hash >> 8) & 31) as i32))
    else {
        return Ok(None);
    };
    let Some(y) = grid[1]
        .checked_mul(128)
        .and_then(|v| v.checked_add(32 + ((hash >> 16) & 31) as i32))
    else {
        return Ok(None);
    };
    let terrain = fields.sample(x, y, settings.rivers);
    let biome = surface_biome_selector::select(seed, [x, y], terrain);
    let Some(style) = CampStyle::for_biome(biome.id()) else {
        return Ok(None);
    };
    assemble(
        style,
        [x, y, i32::from(terrain.height) - 1],
        ((hash >> 24) & 3) as u8,
        hash,
        |[sx, sy]| {
            let t = fields.sample(sx, sy, settings.rivers);
            Some(SurfaceSample {
                ground: i32::from(t.height) - 1,
                water: t.water_level.map(|v| i32::from(v) - 1),
            })
        },
        cancelled,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn whole_foundations_and_all_rotations_project_without_owner_drift() {
        for style in CampStyle::ALL {
            for turns in 0..4 {
                let p = assemble(
                    style,
                    [30, -30, 70],
                    turns,
                    19,
                    |_| {
                        Some(SurfaceSample {
                            ground: 68,
                            water: None,
                        })
                    },
                    || false,
                )
                .unwrap()
                .unwrap()
                .prepared;
                assert!(p.cells().any(|(p, s)| p[2] == 69 && s.is_some()));
                let all: BTreeMap<_, _> = p.cells().map(|(p, s)| (p, s.cloned())).collect();
                let split: BTreeMap<_, _> = p
                    .project([-100, -100], [30, 100])
                    .unwrap()
                    .chain(p.project([30, -100], [100, 100]).unwrap())
                    .map(|(p, s)| (p, s.cloned()))
                    .collect();
                assert_eq!(all, split);
                let floor = if style == CampStyle::WindsweptForest {
                    69
                } else {
                    70
                }; // Only the target style fits its floor.
                assert_eq!(p.anchor(), [30, -30, floor]); // Preserve the fixed anchor for all other styles.
                assert!(p.source().contains(style.name()));
            }
        }
    }
    #[test]
    fn wet_unknown_sloped_or_cancelled_terrain_never_publishes_a_fragment() {
        for sample in [
            None,
            Some(SurfaceSample {
                ground: 60,
                water: Some(61),
            }),
            Some(SurfaceSample {
                ground: 56,
                water: None,
            }),
        ] {
            assert!(
                assemble(CampStyle::Forest, [0, 0, 60], 0, 0, |_| sample, || false)
                    .unwrap()
                    .is_none()
            );
        }
        assert!(matches!(
            assemble(
                CampStyle::Forest,
                [0, 0, 60],
                0,
                0,
                |_| Some(SurfaceSample {
                    ground: 60,
                    water: None
                }),
                || true
            ),
            Err(AssetError::Cancelled)
        ));
    }
}

#[cfg(test)] // Keep regression helpers outside production builds.
mod foundation_tests {
    // Exercise the targeted fit and the unchanged public integration.
    use super::super::surface_biomes::SurfaceBiome; // Verify canonical natural eligibility.
    use super::super::surface_generation::{self, Region, SourceOwner, SurfaceWorld}; // Real consumer.
    use super::*; // Use the actual assembly and candidate entry points.
    use std::cell::{Cell, RefCell}; // Instrument immutable callbacks without changing their values.
    use std::collections::BTreeSet; // Detect duplicate terrain sampling.
    fn dry(ground: i32) -> SurfaceSample {
        // Inclusive-ground synthetic fixture only.
        SurfaceSample {
            ground,
            water: None,
        } // No invented water or biome acceptance.
    } // End fixture constructor.
    fn turn_xy(mut xy: [i32; 2], turns: u8) -> [i32; 2] {
        // Independent iterative rotation oracle.
        for _ in 0..turns % 4 {
            // Do not call the production coordinate helper.
            xy = [-xy[1], xy[0]]; // One clockwise scene-ground quarter turn.
        } // End quarter turns.
        xy // Return the rotated offset.
    } // End coordinate oracle.
    fn point(anchor: [i32; 3], local: [i32; 3], turns: u8) -> [i32; 3] {
        // Small test coordinates.
        let xy = turn_xy([local[0], local[1]], turns); // Rotate only the ground plane.
        [anchor[0] + xy[0], anchor[1] + xy[1], anchor[2] + local[2]] // Preserve elevation.
    } // End test point transform.
    fn ramp(anchor: [i32; 3], turns: u8) -> impl Fn([i32; 2]) -> Option<SurfaceSample> {
        // Five-block relief.
        move |xy| {
            // Transform global callbacks into the test's source-local terrain.
            let local = turn_xy([xy[0] - anchor[0], xy[1] - anchor[1]], (4 - turns % 4) % 4); // Inverse.
            Some(dry(80 + local[0].div_euclid(5))) // Both cuts and fills with dry central access.
        } // End stable terrain callback.
    } // End ramp fixture.
    fn cells(prepared: &Prepared<BlockState>) -> BTreeMap<[i32; 3], Option<BlockState>> {
        // Retain air.
        prepared
            .cells()
            .map(|(position, state)| (position, state.cloned()))
            .collect() // Own comparisons.
    } // End snapshot helper.
    fn region(prepared: &Prepared<BlockState>) -> Region {
        // Independent bounds from complete writes.
        let mut minimum = [i32::MAX; 2]; // Include rotated negative offsets.
        let mut maximum = [i32::MIN; 2]; // Exclusive upper bounds.
        for (position, _) in prepared.cells() {
            // Include explicit air, not just solids.
            for axis in 0..2 {
                // Only ground-plane clipping is supported.
                minimum[axis] = minimum[axis].min(position[axis]); // Small test coordinates only.
                maximum[axis] = maximum[axis].max(position[axis] + 1); // Preserve the last column.
            } // End ground axes.
        } // End complete footprint traversal.
        Region { minimum, maximum } // All callers use representable bounded test regions.
    } // End bounds helper.
    #[test] // All authored shelter variants and fire layouts, under every rotation.
    fn windswept_rotated_supports_states_air_and_projection_match_the_source() {
        // No flat-only success.
        let nominal = [-17, -33, 80]; // Cross negative world and sixteen-block boundaries.
        for seed in 0..40 {
            // Covers ten shelter variants, four fire layouts, and optional third tents.
            for turns in 0..4 {
                // No rotation is exempt from the foundation policy.
                let camp = assemble(
                    CampStyle::WindsweptForest,
                    nominal,
                    turns,
                    seed,
                    ramp(nominal, turns),
                    || false,
                )
                .unwrap()
                .unwrap(); // Production.
                let prepared = &camp.prepared; // Keep the real prepared identity.
                let anchor = [-17, -33, 83]; // Derived from this synthetic ramp's feasible floor interval.
                assert_eq!(prepared.anchor(), anchor); // A corner-based unchanged floor is incorrect here.
                assert_eq!(prepared.rotation(), turns); // Preserve the selected rotation.
                let mut kit =
                    surface_camps::build(CampStyle::WindsweptForest, seed, state).unwrap(); // Original geometry.
                assert_eq!(prepared.source(), kit.template.source); // Keep the complete seed/style identity.
                let mut expected = BTreeMap::new(); // Independent global write oracle.
                for cell in &kit.template.cells {
                    // Check every original solid and every original air cell.
                    expected.insert(
                        point(anchor, cell.position, turns),
                        cell.state
                            .as_ref()
                            .map(|block| block.rotated(turns).unwrap()),
                    ); // Separate state adapter.
                } // End authored-state comparison inputs.
                for y in 0..28 {
                    // Complete original footprint, not selected support posts.
                    for x in 0..28 {
                        // The ramp is intentionally not rotation-invariant in global space.
                        for z in 81 + x / 5..83 {
                            // Inclusive ground is 80+x/5; supports stop below floor83.
                            let local = [x, y, z - 83]; // Negative source-local support elevation.
                            let support = state("minecraft:dirt", &[]).unwrap(); // Required Windswept support soil.
                            expected.insert(point(anchor, local, turns), Some(support.clone())); // Detect wrongly rotated supports.
                            kit.template.cells.push(TemplateCell {
                                position: local,
                                state: Some(support),
                            }); // Complete independent template.
                        } // End support elevation oracle.
                    } // End support X oracle.
                } // End support Y oracle.
                let actual = cells(prepared); // Includes all reserved air.
                assert_eq!(actual, expected); // Reject missing air, extra cuts, changed props, or floating supports.
                assert_eq!(actual.len(), 28 * 28 * 8 + 420); // The ramp requires exactly420 below-floor blocks.
                assert!(actual
                    .iter()
                    .all(|(position, value)| position[2] >= 83 || value.is_some())); // No underground air.
                let fire = actual
                    .values()
                    .flatten()
                    .find(|block| block.id().as_str() == "minecraft:campfire")
                    .unwrap(); // Actual transformed state.
                assert_eq!(
                    fire.property("facing"),
                    Some(["south", "west", "north", "east"][usize::from(turns)])
                ); // Explicit direction oracle.
                assert_eq!(fire.property("lit"), Some("false")); // Never turn the abandoned fire on.
                assert_eq!(fire.property("waterlogged"), Some("false")); // Dry-state identity survives.
                let chest = actual
                    .values()
                    .flatten()
                    .find(|block| block.id().as_str() == "minecraft:chest")
                    .unwrap(); // Authored storage.
                assert_eq!(
                    chest.property("facing"),
                    Some(["west", "north", "east", "south"][usize::from(turns)])
                ); // Explicit chest rotation.
                assert_eq!(chest.property("type"), Some("single")); // Rotation must not change storage halves.
                let bench_axis = ["x", "z", "x", "z"][usize::from(turns)]; // Java vertical remains Y.
                assert_eq!(
                    actual
                        .values()
                        .flatten()
                        .filter(|block| block.id().as_str() == "minecraft:spruce_log"
                            && block.property("axis") == Some(bench_axis))
                        .count(),
                    3
                ); // Three horizontal bench logs.
                for x in 12..=14 {
                    // Verify the source-grounded central access lane.
                    for y in 0..=1 {
                        // Connect the external apron to the existing internal route.
                        assert!(actual[&point(anchor, [x, y, 0], turns)].is_some()); // Supported lane floor.
                        for z in 1..=2 {
                            // Full walking clearance, not merely an absent source position.
                            assert_eq!(actual[&point(anchor, [x, y, z], turns)], None);
                            // Explicit retained air.
                        } // End walking clearance.
                    } // End lane connection.
                } // End central access checks.
                let bounds = region(prepared); // Obtain the complete rotated clearing bounds.
                let seam = (bounds.minimum[0].div_euclid(16) + 1) * 16; // A real chunk boundary inside28 columns.
                let halves: BTreeMap<_, _> = prepared
                    .project(
                        bounds.minimum.map(i64::from),
                        [i64::from(seam), i64::from(bounds.maximum[1])],
                    )
                    .unwrap() // Left half.
                    .chain(
                        prepared
                            .project(
                                [i64::from(seam), i64::from(bounds.minimum[1])],
                                bounds.maximum.map(i64::from),
                            )
                            .unwrap(),
                    ) // Right half.
                    .map(|(position, value)| (position, value.cloned()))
                    .collect(); // Keep air in the union.
                assert_eq!(halves, actual); // Projection cannot omit or duplicate seam writes.
                let shifted = Region {
                    minimum: [bounds.minimum[0] + 5, bounds.minimum[1] + 3],
                    maximum: bounds.maximum,
                }; // Partial retained window.
                let clipped: BTreeMap<_, _> = prepared
                    .project(
                        shifted.minimum.map(i64::from),
                        shifted.maximum.map(i64::from),
                    )
                    .unwrap()
                    .map(|(p, s)| (p, s.cloned()))
                    .collect(); // Real projection.
                let filtered: BTreeMap<_, _> = actual
                    .iter()
                    .filter(|(p, _)| shifted.contains(**p))
                    .map(|(p, s)| (*p, s.clone()))
                    .collect(); // Independent predicate.
                assert_eq!(clipped, filtered); // Shifted windows retain exact states and air.
                assert_eq!(prepared.anchor(), anchor); // Projection never rebases the source owner.
                assert_eq!(prepared.rotation(), turns); // Projection never changes source rotation.
                for local in [[27, 27, 7], [0, 0, -1]] {
                    // A reserved air cell and a real added support.
                    let blocked = point(anchor, local, turns); // Block even an offscreen extremity atomically.
                    for habitat in [Habitat::Unknown, Habitat::Protected] {
                        // Both existing rejection guards.
                        let rejected = Prepared::prepare(
                            &kit.template,
                            anchor,
                            turns,
                            |block, n| block.rotated(n).map_err(|_| PlacementError::InvalidState),
                            |p| {
                                if p == blocked {
                                    habitat
                                } else {
                                    Habitat::Replaceable
                                }
                            },
                            || false,
                        ); // Real boundary.
                        assert!(
                            matches!(rejected, Err(PlacementError::Unknown(p) | PlacementError::Protected(p)) if p == blocked)
                        ); // No clipped salvage.
                    } // End habitat rejection cases.
                } // End full-reservation rejection checks.
            } // End rotation coverage.
        } // End source-layout coverage.
    } // End exact source/support/projection regression.
    #[test] // Exercise each independent terrain safety rejection before publication.
    fn windswept_rejects_unknown_wet_cliffs_relief_access_work_and_height() {
        // No arbitrary threshold widening.
        for case in 0..14 {
            // Distinct adverse fixtures are labelled by their case number.
            let placement = assemble(
                CampStyle::WindsweptForest,
                [0, 0, 80],
                0,
                19,
                |xy| {
                    // Public target path.
                    let mut ground = dry(80); // A valid known dry control surface.
                    match case {
                        // Perturb only the named safety property.
                        0 if xy == [27, 27] => return None, // Unknown far corner rejects the whole clearing.
                        1 if xy == [27, 27] => ground.water = Some(81), // Wet far corner is not filled over.
                        2 if xy == [13, -1] => return None, // Unknown external access is not inferred.
                        3 if xy == [13, -1] => ground.water = Some(81), // Wet external access is not bridged.
                        4 if xy == [27, 27] => ground.ground = 83, // Three-block neighbor step fails.
                        5 => ground.ground = if xy[1] == -1 { 88 } else { 80 + xy[0] / 3 }, // Nine-block broad relief fails.
                        6 if xy[1] == -1 => ground.ground = 70, // Access would require excessive cutting.
                        7 if xy[1] == -1 => ground.ground = 89, // Access would require excessive below-floor fill.
                        8 if xy[1] == -1 => ground.ground = 86, // Four fill blocks per column exceed total work2352.
                        9 => ground.ground = -1, // Missing supporting solid below global zero.
                        10 => ground.ground = i32::MAX, // Reject before arithmetic can overflow.
                        11 => ground.ground = i32::MIN, // Reject before subtraction or absolute value.
                        12 => ground.ground = 253, // Seven reserved levels cannot fit above the admissible floor.
                        13 if xy == [27, 27] => {
                            ground = SurfaceSample {
                                ground: 82,
                                water: Some(82),
                            }
                        } // Dry datum forbids the required cut.
                        _ => {}                    // Keep all other columns unchanged.
                    } // End adverse fixture selection.
                    Some(ground) // Return the immutable per-coordinate observation.
                },
                || false,
            )
            .unwrap(); // Ordinary terrain rejection is not an asset error.
            assert!(placement.is_none(), "unsafe case {case} accepted"); // A fragment must never escape.
        } // End rejection inventory.
    } // End adverse-terrain regression.
    #[test] // Accept exact safe boundaries rather than rejecting every non-flat site.
    fn windswept_cut_fill_and_vertical_limits_are_inclusive() {
        // Check the selected policy numerically.
        let filled = assemble(
            CampStyle::WindsweptForest,
            [0, 0, 87],
            0,
            19,
            |xy| {
                // Six-block support limit.
                Some(dry(if xy[1] == -1 {
                    87
                } else {
                    80 + 2 * xy[0].min(4)
                })) // Relief8 and neighbor step2.
            },
            || false,
        )
        .unwrap()
        .unwrap()
        .prepared; // A complete non-flat accepted fixture.
        assert_eq!(filled.anchor()[2], 87); // Minimum-work feasible floor.
        let writes = cells(&filled); // Inspect the actual support stack.
        for z in 81..87 {
            // Exactly six added blocks below the floor.
            assert_eq!(
                writes[&[0, 0, z]].as_ref().unwrap().id().as_str(),
                "minecraft:dirt"
            ); // No gap.
        } // End exact fill boundary.
        assert!(!writes.contains_key(&[0, 0, 80])); // Existing terrain is not relabelled as support.
        let cut = assemble(
            CampStyle::WindsweptForest,
            [0, 0, 85],
            0,
            19,
            |xy| {
                // Two-block cut limit.
                Some(dry(if xy == [27, 27] {
                    87
                } else if xy[1] == -1 {
                    84
                } else {
                    85
                })) // Local step2 only.
            },
            || false,
        )
        .unwrap()
        .unwrap()
        .prepared; // Access forces a safe floor85.
        assert_eq!(cut.anchor()[2], 85); // Do not raise this into an inaccessible pad.
        let writes = cells(&cut); // Inspect excavation outside any shelter geometry.
        assert_eq!(writes[&[27, 27, 86]], None); // First removed terrain solid.
        assert_eq!(writes[&[27, 27, 87]], None); // Second removed terrain solid.
        for (ground, nominal, floor) in [(0, 0, 1), (249, 249, 249)] {
            // Lower support and upper reservation limits.
            let p = assemble(
                CampStyle::WindsweptForest,
                [0, 0, nominal],
                0,
                19,
                |_| Some(dry(ground)),
                || false,
            )
            .unwrap()
            .unwrap()
            .prepared; // Public boundary.
            assert_eq!(p.anchor()[2], floor); // Actual fitted elevation.
            assert_eq!(p.cells().map(|(p, _)| p[2]).max(), Some(floor + 7)); // All reserved air counts toward bounds.
            assert!(p.cells().all(|(p, _)| (0..=256).contains(&p[2]))); // No escaped vertical write.
        } // End vertical boundary cases.
        let at_budget = assemble(
            CampStyle::WindsweptForest,
            [0, 0, 80],
            0,
            19,
            |xy| Some(dry(if xy[1] == -1 { 85 } else { 80 })),
            || false,
        )
        .unwrap()
        .unwrap()
        .prepared; // Exactly2352 added supports.
        assert_eq!(at_budget.anchor()[2], 84); // Access forces three below-floor blocks in every column.
        assert_eq!(at_budget.cells().count(), 8624); // Complete6272-cell template plus the exact allowed work budget.
        let at_datum = assemble(
            CampStyle::WindsweptForest,
            [0, 0, 80],
            0,
            19,
            |_| {
                Some(SurfaceSample {
                    ground: 80,
                    water: Some(80),
                })
            },
            || false,
        )
        .unwrap()
        .unwrap()
        .prepared; // Equality is dry.
        assert_eq!(at_datum.anchor()[2], 80); // Preserve inclusive water semantics.
    } // End exact-boundary regression.
    #[test] // The new branch must not relax or refit any of the other seventeen styles.
    fn non_windswept_styles_keep_the_original_admission_policy() {
        // Exercise all original variants.
        let anchor = [-17, -33, 80]; // A fixed old-policy anchor.
        for style in CampStyle::ALL {
            // Do not silently omit a supported style.
            if style == CampStyle::WindsweptForest {
                // Its new behavior has dedicated coverage above.
                continue; // Preserve the old-policy assertions for exactly seventeen styles.
            } // End target-style exclusion.
            for turns in 0..4 {
                // Other styles retain every original rotation.
                assert!(
                    assemble(style, anchor, turns, 19, ramp(anchor, turns), || false)
                        .unwrap()
                        .is_none()
                ); // Old two-block rule still rejects.
                let p = assemble(style, anchor, turns, 19, |_| Some(dry(78)), || false)
                    .unwrap()
                    .unwrap()
                    .prepared; // Original short-support success.
                assert_eq!(p.anchor(), anchor); // Other styles are never fitted to a new plane.
                let soil = if style == CampStyle::WoodedBadlands {
                    "minecraft:red_sandstone"
                } else {
                    "minecraft:dirt"
                }; // Original materials.
                for (_, value) in p.cells().filter(|(p, _)| p[2] == 79) {
                    // Inspect every old support block.
                    assert_eq!(value.unwrap().id().as_str(), soil); // No recolor or missing state.
                } // End legacy-support checks.
                assert_eq!(p.cells().filter(|(p, _)| p[2] == 79).count(), 784); // Complete support layer retained.
            } // End original-rotation coverage.
        } // End original-style coverage.
        for biome in SurfaceBiome::all() {
            // Exact canonical registry eligibility remains unchanged.
            let expected = CampStyle::ALL
                .into_iter()
                .find(|style| style.biome_id() == biome.id()); // Independent registry equality.
            assert_eq!(CampStyle::for_biome(biome.id()), expected); // No aliases or blanket woodland admission.
        } // End biome-table checks.
        assert_eq!(CampStyle::for_biome("minecraft:windswept_hills"), None); // Do not broaden the repaired family.
    } // End unchanged-policy regression.
    #[test] // Check sampling bounds and cancellation throughout actual target assembly.
    fn windswept_sampling_cancellation_and_coordinate_bounds_are_atomic() {
        // No unbounded retries.
        let sampled = RefCell::new(BTreeSet::new()); // Count unique global callback positions.
        let polls = Cell::new(0_usize); // Capture cancellation checkpoints without cancelling.
        let build = || {
            assemble(
                CampStyle::WindsweptForest,
                [0, 0, 80],
                3,
                19,
                |xy| {
                    // Nonzero rotation.
                    assert!(sampled.borrow_mut().insert(xy)); // Every footprint/approach column is sampled once.
                    Some(dry(80)) // Stable values despite test bookkeeping.
                },
                || {
                    polls.set(polls.get() + 1);
                    false
                },
            )
        }; // Count every cancellation check.
        assert!(build().unwrap().is_some()); // Establish a complete successful control.
        assert_eq!(sampled.borrow().len(), 787); // Exactly784 footprint plus3 read-only approach columns.
        let total = polls.get(); // Exercise late cancellation without invented implementation counts.
        for stop in [1, 400, 788, total / 2, total - 1, total] {
            // Early, planning, preparation, final publication.
            polls.set(0); // Reset independently for each cancellation attempt.
            let result = assemble(
                CampStyle::WindsweptForest,
                [0, 0, 80],
                3,
                19,
                |_| Some(dry(80)),
                || {
                    polls.set(polls.get() + 1);
                    polls.get() >= stop
                },
            ); // Deterministic cancellation.
            assert!(matches!(result, Err(AssetError::Cancelled)), "stop={stop}");
            // Cancellation is never ordinary absence.
        } // End cancellation stages.
        for (anchor, turns) in [
            ([i32::MAX, 0, 80], 0),
            ([i32::MIN, 0, 80], 1),
            ([0, i32::MAX, 80], 0),
            ([0, i32::MIN, 80], 2),
            ([0, i32::MIN, 80], 0),
            ([0, 0, -1], 0),
            ([0, 0, 250], 0),
        ] {
            // Footprint, approach, and nominal-height overflow.
            assert!(assemble(
                CampStyle::WindsweptForest,
                anchor,
                turns,
                19,
                |_| Some(dry(80)),
                || false
            )
            .unwrap()
            .is_none()); // Reject without wrapping.
        } // End extreme-coordinate cases.
        let fields = TerrainFields::new(71839); // The ordinary natural candidate boundary.
        let settings = VoxelLandscapeSettings::default(); // Do not invent a special admission mode.
        assert!(
            candidate([i32::MAX, i32::MIN], &fields, &settings, || false)
                .unwrap()
                .is_none()
        ); // Checked grid scaling.
        assert!(matches!(
            candidate([i32::MAX, i32::MIN], &fields, &settings, || true),
            Err(AssetError::Cancelled)
        )); // Cancellation has priority.
    } // End atomicity regression.
    fn assert_world(world: &SurfaceWorld, camp: &CampPlacement) {
        // Verify the real whole-generator consumer.
        let prepared = &camp.prepared; // Retain complete writes for an independent region filter.
        let owner = SourceOwner::Structure {
            anchor: prepared.anchor(),
            source: format!("{}/rotation/{}", prepared.source(), prepared.rotation()),
        }; // Exact production owner contract.
        let (mut projected, mut solids) = (0_usize, 0_usize); // Count air and blocks separately.
        for (position, expected) in prepared.cells() {
            // Never derive acceptance from a clipped template.
            if !world.region.contains(position) {
                // Compare only this world's actual region.
                continue; // The full reservation still determines the owner and acceptance.
            } // End projection filter.
            projected += 1; // Explicit air contributes to the feature receipt.
            assert!(!world.fluids.contains_key(&position)); // No submerged foundation or interior fluid.
            let Some(expected) = expected else {
                // Explicit air must remain clear after all later writers.
                assert!(
                    !world.blocks.contains_key(&position),
                    "reserved air lost at {position:?}"
                ); // No surviving terrain/tree/plant.
                continue; // Do not confuse air with an absent prepared coordinate.
            }; // End air assertion.
            solids += 1; // Count the exact authored/support solids.
            let actual = world.blocks.get(&position).expect("missing camp solid"); // Physical write must exist.
            assert_eq!(&actual.state, expected); // Preserve every state property, including rotation.
            assert_eq!(actual.owner, owner); // Preserve fitted anchor and seed/rotation source identity.
        } // End complete-write comparison.
        assert!(projected > 0 && solids > 0); // Empty windows are not witnesses.
        assert_eq!(
            world
                .blocks
                .values()
                .filter(|block| block.owner == owner)
                .count(),
            solids
        ); // No additional owner-labelled writes.
        let records: Vec<_> = world
            .structures
            .iter()
            .filter(|record| {
                record.source == camp.style.source_id() && record.anchor == prepared.anchor()
            })
            .collect(); // Exact family receipt.
        assert_eq!(records.len(), 1); // No duplicate or absent structure record.
        assert_eq!(records[0].projected_cells, projected); // Count explicit air as well as solids.
        assert!(records[0].authored_placement); // Do not relabel the placement as native generation.
    } // End whole-generator oracle.
    #[test] // A bounded discovery test, not fabricated fixed passing coordinates.
    fn natural_windswept_foundation_survives_whole_split_and_shifted_worlds() {
        // Mandatory natural regression.
        let settings = VoxelLandscapeSettings {
            seed: 71839,
            structures_percent: 100,
            ..Default::default()
        }; // Original diagnostic conditions.
        let fields = TerrainFields::new(u64::from(settings.seed)); // Same immutable production terrain.
        let (mut eligible, mut assembled, mut negative) = (0_usize, 0_usize, 0_usize); // Useful bounded failure evidence.
        for gy in -256..=256 {
            // Frozen diagnostic search domain only.
            for gx in -256..=256 {
                // No anchor relocation or artificially increased density.
                let grid = [gx, gy]; // Original candidate grid.
                let hash = hash2(71839 ^ 0x6361_6d70_3030_3031, i64::from(gx), i64::from(gy)); // Observational prefilter.
                if hash % 100 >= 12 {
                    // Skip unrelated density rejections without changing the gate.
                    continue; // The actual candidate is still called below.
                } // End hash prefilter.
                let x = gx * 128 + 32 + ((hash >> 8) & 31) as i32; // Exact bounded candidate X.
                let y = gy * 128 + 32 + ((hash >> 16) & 31) as i32; // Exact bounded candidate Y.
                let terrain = fields.sample(x, y, settings.rivers); // No synthetic flattened terrain.
                if surface_biome_selector::select(71839, [x, y], terrain)
                    != SurfaceBiome::WindsweptForest
                {
                    // Canonical eligibility.
                    continue; // Never substitute another biome's camp.
                } // End biome prefilter.
                eligible += 1; // Track real attempted target-family anchors.
                let Some(camp) = candidate(grid, &fields, &settings, || false).unwrap() else {
                    // Authoritative production admission.
                    continue; // Keep legitimate terrain rejection.
                }; // End candidate absence handling.
                assembled += 1; // Candidate success alone is not whole-generator acceptance.
                assert_eq!(camp.style, CampStyle::WindsweptForest); // The test must observe the missing family.
                let p = &camp.prepared; // Inspect the exact production placement.
                assert_eq!(&p.anchor()[..2], &[x, y]); // No horizontal retries or forced relocation.
                assert_eq!(p.rotation(), ((hash >> 24) & 3) as u8); // Natural rotation gate is unchanged.
                if x >= 0 && y >= 0 {
                    // Require negative-coordinate natural window coverage too.
                    continue; // Synthetic tests already exercise all four rotations.
                } // End natural witness location constraint.
                negative += 1; // Distinguish lack of negative witnesses from generator collisions.
                let bounds = region(p); // Use the complete rotated footprint, not the nominal corner.
                let world = surface_generation::prepare(bounds, &settings, || false).unwrap(); // Ordinary complete generator.
                if !world.structures.iter().any(|record| {
                    record.source == camp.style.source_id() && record.anchor == p.anchor()
                }) {
                    // Whole overlap admission.
                    continue; // Do not waive collisions to produce a witness.
                } // End whole-generator admission check.
                assert_world(&world, &camp); // Check all final states, air, fluids, owners, and receipts.
                let floor = p.anchor()[2]; // Actual fitted floor, not the nominal corner height.
                let (mut work, mut fills, mut old_delta) = (0_usize, 0_usize, 0_i32); // Independent numeric checks.
                let mut heights = BTreeMap::<[i32; 2], i32>::new(); // Preserve source-local adjacency for the oracle.
                for local_y in 0..28 {
                    // Check all original columns after whole-world generation.
                    for local_x in 0..28 {
                        // Rotated support lookup must match source-local footprint.
                        let position = point(p.anchor(), [local_x, local_y, 0], p.rotation()); // Independent transform.
                        let t = fields.sample(position[0], position[1], settings.rivers); // Real terrain, no fixture.
                        assert!(t.water_level.is_none_or(|level| level <= t.height)); // Canonical dry eligibility.
                        let ground = i32::from(t.height) - 1; // Inclusive support convention.
                        if let Some(water) = t.water_level {
                            // Dry datum still constrains a cut.
                            assert!(floor >= i32::from(water) - 1); // No cut below recorded water.
                        } // End datum check.
                        let fill = (floor - ground - 1).max(0) as usize; // Added blocks below the authored floor.
                        let cut = (ground - floor).max(0) as usize; // Removed solids above the floor.
                        assert!(fill <= 6 && cut <= 2); // Exact selected per-column limits.
                        work += fill + cut; // Independent whole-earthwork sum.
                        fills += fill; // Distinguish added supports from already-reserved air cuts.
                        old_delta = old_delta.max((ground - (i32::from(terrain.height) - 1)).abs()); // Original rejection predicate.
                        for z in ground.min(floor - 1)..=floor {
                            // Terrain base, supports, and floor must be contiguous.
                            assert!(
                                world.blocks.contains_key(&[position[0], position[1], z]),
                                "unsupported floor at {position:?}, z={z}"
                            ); // Physical support.
                        } // End support-stack assertion.
                        heights.insert([local_x, local_y], ground); // Save for independent local-relief checks.
                    } // End source-local X checks.
                } // End source-local Y checks.
                let relief = heights.values().max().unwrap() - heights.values().min().unwrap(); // Complete actual relief.
                assert!(
                    old_delta > 2,
                    "witness did not exercise the former rejection"
                ); // A pre-existing flat success is insufficient.
                assert!(relief <= 8 && work <= 2352); // Use literal policy bounds rather than production constants.
                assert!((1..=249).contains(&floor)); // Seven retained levels remain in the world.
                assert_eq!(p.cells().count(), 28 * 28 * 8 + fills); // No missing or extra reservation cells.
                for (&[local_x, local_y], &ground) in &heights {
                    // Verify both cardinal neighbor directions.
                    for neighbor in [[local_x - 1, local_y], [local_x, local_y - 1]] {
                        // Independent local adjacency.
                        if let Some(other) = heights.get(&neighbor) {
                            // Skip only outside-footprint positions.
                            assert!((ground - other).abs() <= 2); // No admitted local cliff.
                        } // End neighbor assertion.
                    } // End neighbor directions.
                } // End local-relief checks.
                for local_x in 12..=14 {
                    // Check all three rotated external approach columns.
                    let position = point(p.anchor(), [local_x, -1, 0], p.rotation()); // No production-coordinate helper.
                    let t = fields.sample(position[0], position[1], settings.rivers); // Original unchanged terrain.
                    assert!(t.water_level.is_none_or(|level| level <= t.height)); // Dry access is mandatory.
                    assert!((floor - (i32::from(t.height) - 1)).abs() <= 1); // At most a one-block external step.
                } // End natural approach checks.
                let seam = (bounds.minimum[0].div_euclid(16) + 1) * 16; // Genuine negative-coordinate chunk boundary.
                for window in [
                    bounds,
                    Region {
                        minimum: [seam, bounds.minimum[1]],
                        maximum: bounds.maximum,
                    },
                    Region {
                        minimum: bounds.minimum,
                        maximum: [seam, bounds.maximum[1]],
                    },
                    Region {
                        minimum: [bounds.minimum[0] + 5, bounds.minimum[1] + 3],
                        maximum: [bounds.maximum[0] + 5, bounds.maximum[1] + 3],
                    },
                ] {
                    // Repeat, reversed split order, and shifted window.
                    assert_world(
                        &surface_generation::prepare(window, &settings, || false).unwrap(),
                        &camp,
                    ); // No window-dependent identity or air loss.
                } // End real-generator continuity checks.
                let again = candidate(grid, &fields, &settings, || false)
                    .unwrap()
                    .unwrap(); // Recompute deterministically.
                assert_eq!(again.prepared.anchor(), p.anchor()); // Same fitted elevation.
                assert_eq!(again.prepared.source(), p.source()); // Same source seed/style.
                assert_eq!(again.prepared.rotation(), p.rotation()); // Same rotation identity.
                assert_eq!(cells(&again.prepared), cells(p)); // Same complete states and air.
                let dense = VoxelLandscapeSettings {
                    structures_percent: 200,
                    ..settings.clone()
                }; // Density cannot relocate an already-selected candidate.
                let dense = candidate(grid, &fields, &dense, || false).unwrap().unwrap(); // Actual production density gate.
                assert_eq!(cells(&dense.prepared), cells(p)); // Same source layout and support decision.
                assert_eq!(dense.prepared.anchor(), p.anchor()); // Same complete owner anchor.
                let disabled = VoxelLandscapeSettings {
                    structures_percent: 0,
                    ..settings.clone()
                }; // Zero means no camp.
                assert!(candidate(grid, &fields, &disabled, || false)
                    .unwrap()
                    .is_none()); // No repair-specific bypass.
                let without = surface_generation::prepare(bounds, &disabled, || false).unwrap(); // Check the ordinary consumer too.
                assert!(!without
                    .structures
                    .iter()
                    .any(|record| record.source == camp.style.source_id())); // No hidden forced camp.
                assert!(matches!(
                    candidate(grid, &fields, &settings, || true),
                    Err(AssetError::Cancelled)
                )); // Preserve candidate cancellation.
                assert!(matches!(
                    surface_generation::prepare(bounds, &settings, || true),
                    Err(AssetError::Cancelled)
                )); // Preserve whole-world cancellation.
                eprintln!("D4.CAMP seed=71839 grid={grid:?} anchor={:?} rotation={} old_delta={old_delta} relief={relief} work={work} fills={fills} writes={}", p.anchor(), p.rotation(), p.cells().count()); // Emit only after every assertion passed.
                return; // One verified natural whole-generator witness satisfies this regression.
            } // End bounded grid X search.
        } // End bounded grid Y search.
        panic!("no whole-generator negative-coordinate WindsweptForest witness within radius256: eligible={eligible}, assembled={assembled}, negative={negative}");
        // Failure is not waived or labelled acceptance.
    } // End mandatory natural regression.
} // End camp foundation regressions.
