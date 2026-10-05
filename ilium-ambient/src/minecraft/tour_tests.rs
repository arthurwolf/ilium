//! Synthetic source integration and domain controls; no save reads or renderer.
use super::super::{
    chunk,
    nbt::{Compound, Document, Tag},
    surface,
};
use super::*;
use std::{cell::Cell, collections::BTreeSet};

#[test]
fn failed_route_corridor_skips_jitter_but_keeps_other_maps_directions_and_lengths() {
    let failed = RouteKey {
        map: MapId([1; 16]),
        endpoints: [[-400, 0], [400, 0]],
    };
    let excluded = BTreeSet::from([failed]);
    let near = RouteKey {
        endpoints: [[-350, 20], [450, 20]],
        ..failed
    };
    assert!(near_failed_route(near, &excluded));
    assert!(!near_failed_route(
        RouteKey {
            map: MapId([2; 16]),
            ..near
        },
        &excluded
    ));
    assert!(!near_failed_route(
        RouteKey {
            endpoints: [[-400, 0], [0, 400]],
            ..failed
        },
        &excluded
    ));
    assert!(!near_failed_route(
        RouteKey {
            endpoints: [[-320, 0], [320, 0]],
            ..failed
        },
        &excluded
    ));
}

#[test]
fn projected_receipt_uses_qualified_state_and_current_view_without_losing_planner_targets() {
    use super::super::source_footprint;
    use crate::voxel_landscape::assets::budget::{ByteBudget, Cancel};
    use std::sync::atomic::AtomicBool;

    let base = basic(&[([0, 0], Category::OpenGrassland)]);
    assert!(!base.targets().is_empty());
    let mut controller = Controller::new(1, History::default()).unwrap();
    let plan = choose(&mut controller, std::slice::from_ref(&base), policy(8.0));
    let account = ByteBudget::new(256 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let size = [1, 1];
    let scale = 1024.0;
    let request = Arc::new(
        source_footprint::request(plan.line(), plan.focus_y(), size, scale, &account, cancel)
            .unwrap(),
    );
    let mut decoded_source = loaded(request.support_chunks());
    for (&position, chunk) in &base.loaded().chunks {
        if decoded_source.chunks.contains_key(&position) {
            decoded_source
                .chunks
                .insert(position, Arc::new((**chunk).clone()));
        }
    }
    let map = Arc::new(
        base.projected_source(
            Arc::new(decoded_source),
            account.reserve(1, cancel).unwrap(),
            &mut budget(),
        )
        .unwrap(),
    );
    assert!(map.targets().is_empty());
    let display = Arc::new(
        ProjectedDisplay::bind(
            &plan,
            Arc::clone(&map),
            Arc::clone(&request),
            size,
            scale,
            &mut budget(),
        )
        .unwrap(),
    );
    let view = controller
        .start_projected(&plan, display, clock(0), &budget())
        .unwrap()
        .unwrap();
    let issued = controller.issued_view().unwrap();
    assert_eq!(
        controller.active.as_ref().unwrap().plan.map.targets(),
        base.targets()
    );
    let position = request
        .render_chunks()
        .iter()
        .filter(|chunk| !base.loaded().chunks.contains_key(*chunk))
        .flat_map(|chunk| {
            let x = chunk[0] * 16 + 8;
            let z = chunk[1] * 16 + 8;
            request
                .column_band([x, z])
                .into_iter()
                .flat_map(move |band| (band[0]..=band[1].min(63)).map(move |y| [x, y, z]))
        })
        .find(|&position| {
            request.may_project_cell(position, [view.look_at[0], view.look_at[2]])
                && map.state(position).is_some_and(|state| !state.is_air())
                && (f64::from(position[0]) - view.look_at[0]).abs() > view.data_radius
        })
        .expect("a qualified saved cell beyond the small planner radius must be visible");
    let owner = DisplayedBlock {
        position,
        state: map.state(position).unwrap(),
        pixels: 2,
        resolved: true,
    };
    assert_eq!(
        controller
            .presented_issued(&issued, &[owner], &mut budget())
            .unwrap()
            .credited_categories,
        0
    );
    assert!(controller.active.as_ref().unwrap().any_saved_pixels);
    let mut distant = owner;
    distant.position[0] += 1024;
    assert!(matches!(
        controller.presented_issued(&issued, &[distant], &mut budget()),
        Err(Error::Invalid(_))
    ));
}

#[test]
fn projected_display_rejects_changed_overlap_before_start() {
    use super::super::source_footprint;
    use crate::voxel_landscape::assets::budget::{ByteBudget, Cancel};
    use std::sync::atomic::AtomicBool;

    let base = basic(&[]);
    let mut controller = Controller::new(1, History::default()).unwrap();
    let plan = choose(&mut controller, std::slice::from_ref(&base), policy(8.0));
    let account = ByteBudget::new(256 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let size = [1, 1];
    let scale = 1024.0;
    let request = Arc::new(
        source_footprint::request(plan.line(), plan.focus_y(), size, scale, &account, cancel)
            .unwrap(),
    );
    let mut loaded = loaded(request.support_chunks());
    let changed = *request
        .support_chunks()
        .iter()
        .find(|chunk| base.loaded().chunks.contains_key(*chunk))
        .unwrap();
    loaded.chunks.insert(
        changed,
        Arc::new(decoded(changed, Some(Category::DrySandySurface), 2835)),
    );
    let map = Arc::new(
        base.projected_source(
            Arc::new(loaded),
            account.reserve(1, cancel).unwrap(),
            &mut budget(),
        )
        .unwrap(),
    );
    assert!(matches!(
        ProjectedDisplay::bind(&plan, map, request, size, scale, &mut budget()),
        Err(Error::Stale)
    ));
    assert!(controller.view().is_none());
    assert_eq!(controller.history(), History::default());
}

fn budget() -> Budget<'static> {
    Budget::new(u64::MAX, &|| false)
}
fn source(id: u8, generation: u64) -> Source {
    Source {
        map: MapId([id; 16]),
        generation,
    }
}
fn policy(radius: f64) -> Policy {
    Policy {
        envelope: Envelope {
            cell_heights: [MIN_Y, MAX_Y],
            viewport_radius: radius,
            horizontal_halo: 0.0,
            upward_overhang: 1.0,
            eye_y: 384.0,
        },
        minimum_length: 48.0,
        maximum_length: 192.0,
        max_line_queries: 4096,
        minimum_confidence: Confidence::Supported,
        minimum_pixels: 2,
    }
}

#[test]
fn tour_core_retains_exact_identity_and_only_inset_support() {
    let full = basic(&[
        ([-2, -2], Category::OpenGrassland),
        ([0, 0], Category::DrySandySurface),
    ]);
    let render_core = surface::Bounds {
        minimum: [-32, -32],
        maximum: [47, 47],
    };
    let tour = full.tour_core(render_core, &mut budget()).unwrap();
    assert_eq!(tour.source(), full.source());
    assert_eq!(tour.loaded().chunks.len(), 9);
    assert!(tour
        .loaded()
        .chunks
        .keys()
        .all(|chunk| (-1..=1).contains(&chunk[0]) && (-1..=1).contains(&chunk[1])));
    assert!(tour.targets().iter().all(|target| {
        target.support.minimum[0] >= -16
            && target.support.maximum[0] <= 31
            && target.support.minimum[2] >= -16
            && target.support.maximum[2] <= 31
    }));
    assert!(tour
        .targets()
        .iter()
        .any(|target| target.key.category == Category::DrySandySurface));
    assert!(tour
        .targets()
        .iter()
        .all(|target| target.key.tile != [-2, -2]));
    assert_eq!(full.loaded().chunks.len(), 49);
    assert!(full
        .targets()
        .iter()
        .any(|target| target.key.tile == [-2, -2]));
}

#[test]
fn a_clipped_render_volume_cannot_select_omitted_surface_or_targets() {
    let map = basic(&[([0, 0], Category::OpenGrassland)]);
    let mut controller = Controller::new(1, History::default()).unwrap();
    let ticket = controller.request(&budget()).unwrap();
    let mut clipped = policy(8.0);
    clipped.envelope.cell_heights = [66, 80];
    let result = select(
        ticket,
        &[map],
        &controller.history(),
        clipped,
        &mut budget(),
    )
    .unwrap();
    assert!(result.plan.is_none());
    assert_eq!(result.audit.desired, 0);
}

#[test]
fn presentation_rejects_saved_owners_outside_the_admitted_cell_band() {
    let map = basic(&[([0, 0], Category::OpenGrassland)]);
    let mut controller = Controller::new(1, History::default()).unwrap();
    let mut clipped = policy(8.0);
    clipped.envelope.cell_heights = [64, 65];
    let plan = choose(&mut controller, std::slice::from_ref(&map), clipped);
    let (view, _) = closest_view(&mut controller, &plan);
    let position = [
        view.look_at[0].floor() as i32,
        63,
        view.look_at[2].floor() as i32,
    ];
    let owner = DisplayedBlock {
        position,
        state: map.state(position).unwrap(),
        pixels: 3,
        resolved: true,
    };
    let before = controller.history();
    assert!(matches!(
        controller.presented(view.tag, &[owner], &mut budget()),
        Err(Error::Invalid(_))
    ));
    assert_eq!(controller.history(), before);
}

#[test]
fn sealed_older_emitted_view_uses_its_own_footprint_after_newer_render() {
    let map = basic(&[]);
    let mut controller = Controller::new(1, History::default()).unwrap();
    let plan = choose(&mut controller, std::slice::from_ref(&map), policy(8.0));
    let first = controller
        .start(&plan, clock(0), &budget())
        .unwrap()
        .unwrap();
    let issued = controller.issued_view().unwrap();
    assert_eq!(issued.view(), first);
    let position = [
        first.look_at[0].floor() as i32,
        63,
        first.look_at[2].floor() as i32,
    ];
    let owner = DisplayedBlock {
        position,
        state: map.state(position).unwrap(),
        pixels: 2,
        resolved: true,
    };
    let mut later = first;
    for step in 1..=20 {
        later = controller
            .advance(plan.ticket(), clock(step * 250), &budget())
            .unwrap();
        if (later.look_at[0] - first.look_at[0]).hypot(later.look_at[2] - first.look_at[2])
            > 2.0 * first.data_radius + 2.0
        {
            break;
        }
    }
    assert_ne!(later.tag, first.tag);
    assert!(
        (later.look_at[0] - first.look_at[0]).hypot(later.look_at[2] - first.look_at[2])
            > 2.0 * first.data_radius + 2.0
    );
    assert_eq!(
        controller.presented(first.tag, &[owner], &mut budget()),
        Err(Error::Stale)
    );
    assert_eq!(
        controller
            .presented_issued(&issued, &[owner], &mut budget())
            .unwrap()
            .credited_categories,
        0
    );
    assert_eq!(
        controller.active.as_ref().unwrap().acknowledged,
        first.tag.sequence
    );
    let newer = controller.issued_view().unwrap();
    controller
        .presented_issued(&newer, &[], &mut budget())
        .unwrap();
    assert_eq!(
        controller.active.as_ref().unwrap().acknowledged,
        later.tag.sequence
    );
    // Re-emitting the same raster can reveal more surviving pixels, but a
    // repeated category/run never earns another History credit.
    controller
        .presented_issued(&issued, &[owner], &mut budget())
        .unwrap();
    assert_eq!(
        controller.active.as_ref().unwrap().acknowledged,
        later.tag.sequence
    );
    controller.cancel_viewport(&budget()).unwrap();
    assert_eq!(
        controller.presented_issued(&issued, &[owner], &mut budget()),
        Err(Error::Stale)
    );
}

#[test]
fn issued_view_cannot_cross_controller_even_with_matching_ticket_and_source() {
    let map = basic(&[]);
    let mut first = Controller::new(1, History::default()).unwrap();
    let mut second = Controller::new(1, History::default()).unwrap();
    let first_plan = choose(&mut first, std::slice::from_ref(&map), policy(8.0));
    let second_plan = choose(&mut second, &[map], policy(8.0));
    first.start(&first_plan, clock(0), &budget()).unwrap();
    second.start(&second_plan, clock(0), &budget()).unwrap();
    let foreign = first.issued_view().unwrap();
    assert_eq!(
        second.presented_issued(&foreign, &[], &mut budget()),
        Err(Error::Stale)
    );
}

#[test]
fn endpoint_tag_stays_sealed_until_its_async_receipt_can_finish() {
    let map = basic(&[]);
    let mut controller = Controller::new(1, History::default()).unwrap();
    let plan = choose(&mut controller, &[map], policy(8.0));
    controller.start(&plan, clock(0), &budget()).unwrap();
    let mut endpoint = None;
    for step in 1..=80 {
        let ms = step * 250;
        let view = controller
            .advance(plan.ticket(), clock(ms), &budget())
            .unwrap();
        if view.motion == Motion::Endpoint {
            endpoint = Some((controller.issued_view().unwrap(), ms));
            break;
        }
    }
    let (issued, ms) = endpoint.expect("fixture reaches admitted endpoint");
    for step in 1..=3 {
        let later = controller
            .advance(plan.ticket(), clock(ms + step * 50), &budget())
            .unwrap();
        assert_eq!(later.motion, Motion::Endpoint);
        assert_eq!(later.tag, issued.view().tag);
    }
    controller
        .presented_issued(&issued, &[], &mut budget())
        .unwrap();
    assert_eq!(
        controller.active.as_ref().unwrap().acknowledged,
        issued.view().tag.sequence
    );
    assert_eq!(
        controller
            .finish(plan.ticket(), clock(ms + 150), &budget())
            .unwrap()
            .completion
            .run,
        plan.ticket().run()
    );
    // Scene branches on view presence before starting the next prepared route.
    assert!(controller.view().is_none());
    assert!(controller.issued_view().is_none());
    let successor = choose(&mut controller, &[basic(&[])], policy(8.0));
    let view = if controller.view().is_some() {
        controller
            .advance(successor.ticket(), clock(ms + 200), &budget())
            .map(Some)
    } else {
        controller.start(&successor, clock(ms + 200), &budget())
    }
    .unwrap()
    .unwrap();
    assert_eq!(view.motion, Motion::Started);
    assert_eq!(successor.ticket().run(), 2);
}
fn clock(ms: u64) -> Clock {
    Clock {
        time: Duration::from_millis(ms),
        local_speed: 64.0,
        frozen: false,
    }
}
fn rectangle(minimum: [i32; 2], maximum: [i32; 2]) -> BTreeSet<[i32; 2]> {
    (minimum[1]..=maximum[1])
        .flat_map(|z| (minimum[0]..=maximum[0]).map(move |x| [x, z]))
        .collect()
}
fn fields(items: Vec<(&str, Tag)>) -> Compound {
    items
        .into_iter()
        .map(|(key, value)| (key.into(), value))
        .collect()
}
fn palette() -> Vec<Tag> {
    [
        ("minecraft:air", vec![]),
        ("minecraft:stone", vec![]),
        ("minecraft:grass_block", vec![("snowy", "false")]),
        ("minecraft:grass", vec![]),
        ("minecraft:cobblestone", vec![]),
        ("minecraft:mossy_cobblestone", vec![]),
        ("mod:machine", vec![("zeta", "雪😀"), ("axis", "z")]),
        ("minecraft:sand", vec![]),
        ("minecraft:dead_bush", vec![]),
        ("minecraft:oak_planks", vec![]),
    ]
    .into_iter()
    .map(|(name, properties)| {
        Tag::Compound(fields(vec![
            ("Name", Tag::String(name.into())),
            (
                "Properties",
                Tag::Compound(
                    properties
                        .into_iter()
                        .map(|(key, value)| (key.into(), Tag::String(value.into())))
                        .collect(),
                ),
            ),
        ]))
    })
    .collect()
}
fn paint(category: Option<Category>, x: usize, y: i32, z: usize) -> usize {
    match category {
        Some(Category::DwellingLikeConstruction) if y == 63 => {
            return if x.is_multiple_of(2) { 9 } else { 4 };
        }
        Some(Category::OpenGrassland) => {
            if y == 65 && x.is_multiple_of(4) && z.is_multiple_of(4) {
                return 3;
            }
            if y == 64 {
                return 2;
            }
        }
        Some(Category::DrySandySurface) => {
            if y == 65 && x % 5 == 2 && z % 5 == 2 {
                return 8;
            }
            if y == 64 {
                return 7;
            }
        }
        Some(Category::WeatheredMasonry)
            if (62..=64).contains(&y) && (4..=11).contains(&x) && (4..=11).contains(&z) =>
        {
            return if x.is_multiple_of(2) { 5 } else { 4 };
        }
        _ => {}
    }
    if y < 64 {
        1
    } else {
        0
    }
}
fn decoded(position: [i32; 2], category: Option<Category>, version: i32) -> chunk::DecodedChunk {
    let palette = palette();
    let mut sections = Vec::new();
    for section_y in -4_i8..=19 {
        let blocks = if [3, 4].contains(&section_y) {
            let mut words = vec![0_u64; 256];
            for index in 0..4096 {
                let value = paint(
                    category,
                    index % 16,
                    i32::from(section_y) * 16 + (index / 256) as i32,
                    index / 16 % 16,
                );
                words[index / 16] |= (value as u64) << ((index % 16) * 4);
            }
            fields(vec![
                (
                    "palette",
                    Tag::List {
                        kind: 10,
                        values: palette.clone(),
                    },
                ),
                (
                    "data",
                    Tag::LongArray(words.into_iter().map(|word| word as i64).collect()),
                ),
            ])
        } else {
            fields(vec![(
                "palette",
                Tag::List {
                    kind: 10,
                    values: vec![palette[usize::from(section_y < 4)].clone()],
                },
            )])
        };
        sections.push(Tag::Compound(fields(vec![
            ("Y", Tag::Byte(section_y)),
            ("block_states", Tag::Compound(blocks)),
        ])));
    }
    let wrapped = version <= 2836;
    let body = fields(vec![
        ("xPos", Tag::Int(position[0])),
        ("zPos", Tag::Int(position[1])),
        ("Status", Tag::String("minecraft:full".into())),
        (
            if wrapped { "Sections" } else { "sections" },
            Tag::List {
                kind: 10,
                values: sections,
            },
        ),
    ]);
    let mut root = if wrapped {
        fields(vec![("Level", Tag::Compound(body))])
    } else {
        body
    };
    root.insert("DataVersion".into(), Tag::Int(version));
    chunk::decode(
        &Document {
            name: "synthetic".into(),
            root,
        },
        position,
        chunk::Limits::default(),
        &|| false,
    )
    .unwrap()
}
fn loaded(positions: &BTreeSet<[i32; 2]>) -> LoadedWindow {
    let template = decoded([0, 0], None, 3218);
    let mut result = LoadedWindow::default();
    for &position in positions {
        // Cloning is fixture construction only, never the production admission path.
        let mut chunk = template.clone();
        chunk.identity.position = position;
        result.chunks.insert(position, Arc::new(chunk));
        result.coverage.chunks.insert(position);
    }
    result
}
fn prepared(
    id: u8,
    generation: u64,
    played: i64,
    positions: &BTreeSet<[i32; 2]>,
    features: &[([i32; 2], Category)],
) -> Arc<PreparedMap> {
    let source = source(id, generation);
    let mut loaded = loaded(positions);
    let mut targets = Vec::new();
    for &(position, category) in features {
        assert!(positions.contains(&position));
        let chunk = decoded(position, Some(category), 2835);
        let bounds = surface::Bounds {
            minimum: position.map(|v| v * 16),
            maximum: position.map(|v| v * 16 + 15),
        };
        let view = surface::SurfaceWindow::overworld(
            bounds,
            0,
            std::slice::from_ref(&chunk),
            surface::Limits::default(),
            &mut surface::Work::new(1000, &|| false),
        )
        .unwrap();
        let report = evidence::analyze(
            &view,
            source,
            evidence::Limits::default(),
            &mut surface::Work::new(1_000_000, &|| false),
        )
        .unwrap();
        assert!(report
            .targets
            .iter()
            .any(|target| target.key.category == category));
        targets.extend(report.targets.iter().map(|target| target.summary()));
        drop(report);
        drop(view);
        loaded.chunks.insert(position, Arc::new(chunk));
    }
    Arc::new(PreparedMap::new(source, played, Arc::new(loaded), targets, &mut budget()).unwrap())
}
fn basic(features: &[([i32; 2], Category)]) -> Arc<PreparedMap> {
    prepared(1, 1, 100, &rectangle([-3, -3], [3, 3]), features)
}
fn choose(controller: &mut Controller, maps: &[Arc<PreparedMap>], policy: Policy) -> Plan {
    let ticket = controller.request(&budget()).unwrap();
    select(ticket, maps, &controller.history(), policy, &mut budget())
        .unwrap()
        .plan
        .unwrap()
}
fn pixels_for<'a>(map: &'a PreparedMap, view: View, all_support: bool) -> Vec<DisplayedBlock<'a>> {
    let mut positions = BTreeSet::new();
    if all_support {
        for target in &map.targets {
            for z in target.support.minimum[2]..=target.support.maximum[2] {
                for x in target.support.minimum[0]..=target.support.maximum[0] {
                    for y in target.support.minimum[1]..=target.support.maximum[1] {
                        if supported_position(target, [x, y, z]) {
                            positions.insert([x, y, z]);
                        }
                    }
                }
            }
        }
    } else {
        // The exact saved center column contributes one final pixel owner.
        let x = view.look_at[0].floor() as i32;
        let z = view.look_at[2].floor() as i32;
        for y in (MIN_Y..=MAX_Y).rev() {
            if map.state([x, y, z]).is_some_and(|state| !state.is_air()) {
                positions.insert([x, y, z]);
                break;
            }
        }
    }
    let mut owners: Vec<_> = positions
        .into_iter()
        .filter_map(|position| {
            if [0, 2].into_iter().any(|axis| {
                f64::from(position[axis]) < (view.look_at[axis] - view.data_radius).floor()
                    || f64::from(position[axis]) > (view.look_at[axis] + view.data_radius).floor()
            }) {
                return None;
            }
            let state = map.state(position).filter(|state| !state.is_air())?;
            Some(DisplayedBlock {
                position,
                state,
                pixels: 4,
                resolved: true,
            })
        })
        .collect();
    owners.sort_unstable_by_key(display_order);
    owners
}
fn closest_view(controller: &mut Controller, plan: &Plan) -> (View, u64) {
    let mut best = (
        controller
            .start(plan, clock(0), &budget())
            .unwrap()
            .unwrap(),
        0,
    );
    let point = plan.target.unwrap().key.anchor;
    let distance = |view: View| {
        (view.look_at[0] - (f64::from(point[0]) + 0.5))
            .hypot(view.look_at[2] - (f64::from(point[2]) + 0.5))
    };
    // Stop near the anchor; no time reset or direct private distance mutation.
    for step in 1..=512 {
        let ms = step * 50;
        let view = controller
            .advance(plan.ticket, clock(ms), &budget())
            .unwrap();
        if distance(view) > distance(best.0) {
            return (view, ms);
        }
        best = (view, ms);
        if distance(view) < 1.7 {
            return best;
        }
    }
    panic!("bounded fixture failed to approach anchor");
}
fn complete(
    controller: &mut Controller,
    plan: &Plan,
    start_ms: u64,
    acknowledge_content: bool,
) -> Finished {
    for step in 1..=256 {
        let ms = start_ms + step * 250;
        let view = controller
            .advance(plan.ticket, clock(ms), &budget())
            .unwrap();
        let owners = if acknowledge_content {
            pixels_for(&plan.map, view, false)
        } else {
            Vec::new()
        };
        controller
            .presented(view.tag, &owners, &mut budget())
            .unwrap();
        if view.motion == Motion::Endpoint {
            return controller
                .finish(plan.ticket, clock(ms), &budget())
                .unwrap();
        }
    }
    panic!("bounded fixture did not complete");
}

// Independent continuous segment/half-open expanded-chunk slab oracle. This
// does not call contains_view or copy line_through's four-block stepping rule.
fn hits_missing(line: Line, radius: f64, chunk: [i32; 2]) -> bool {
    let (mut low, mut high, mut low_closed, mut high_closed) = (0.0_f64, 1.0_f64, true, true);
    for (axis, &coordinate) in chunk.iter().enumerate() {
        let lower = f64::from(coordinate) * 16.0 - radius;
        let upper = f64::from(coordinate) * 16.0 + 16.0 + radius;
        let p = line.start[axis];
        let delta = line.end[axis] - p;
        if delta == 0.0 {
            if p < lower || p >= upper {
                return false;
            }
            continue;
        }
        let (a, b, a_closed, b_closed) = if delta > 0.0 {
            ((lower - p) / delta, (upper - p) / delta, true, false)
        } else {
            ((upper - p) / delta, (lower - p) / delta, false, true)
        };
        if a > low {
            low = a;
            low_closed = a_closed;
        } else if a == low {
            low_closed &= a_closed;
        }
        if b < high {
            high = b;
            high_closed = b_closed;
        } else if b == high {
            high_closed &= b_closed;
        }
    }
    low < high || (low == high && low_closed && high_closed)
}
fn assert_sweep(plan: &Plan) {
    let line = plan.line;
    let radius = plan.policy.envelope.radius();
    let minimum: [i32; 2] = std::array::from_fn(|axis| {
        ((line.start[axis].min(line.end[axis]) - radius) / 16.0).floor() as i32
    });
    let maximum: [i32; 2] = std::array::from_fn(|axis| {
        ((line.start[axis].max(line.end[axis]) + radius) / 16.0).floor() as i32
    });
    for z in minimum[1]..=maximum[1] {
        for x in minimum[0]..=maximum[0] {
            if !plan.map.loaded.coverage.chunks.contains(&[x, z]) {
                assert!(!hits_missing(line, radius, [x, z]));
            }
        }
    }
}

#[test]
fn production_decoder_classifier_and_loader_share_exact_state_without_cloning() {
    let map = basic(&[([0, 0], Category::OpenGrassland)]);
    let original = map.loaded.chunks[&[0, 0]].block_at(map.targets[0].key.anchor);
    let BlockSample::State(original) = original else {
        panic!()
    };
    assert!(std::ptr::eq(
        map.state(map.targets[0].key.anchor).unwrap(),
        original
    ));
    assert_eq!(original.properties["snowy"], "false");
    let mut controller = Controller::new(1, History::default()).unwrap();
    let plan = choose(&mut controller, std::slice::from_ref(&map), policy(16.0));
    assert_eq!(plan.choice(), Choice::NovelAppearance);
    assert_eq!(plan.target().unwrap().source, map.source());
    assert_sweep(&plan);
    assert!(plan.line.length() >= 48.0);
    assert_eq!(Arc::strong_count(&map.loaded.chunks[&[0, 0]]), 1);
}

#[test]
fn odd_even_intent_requires_visible_evidence_not_selection_or_loading() {
    let map = basic(&[
        ([0, 0], Category::OpenGrassland),
        ([-1, 0], Category::WeatheredMasonry),
    ]);
    let mut controller = Controller::new(1, History::default()).unwrap();
    let plan = choose(&mut controller, std::slice::from_ref(&map), policy(24.0));
    assert_eq!(plan.ticket.intent(), Kind::Biome);
    assert_eq!(controller.history(), History::default());
    let (view, ms) = closest_view(&mut controller, &plan);
    controller.presented(view.tag, &[], &mut budget()).unwrap();
    assert_eq!(controller.history(), History::default());
    let finished = complete(&mut controller, &plan, ms, false);
    assert!(!finished.completion.chosen_visible);
    assert!(!finished.completion.novelty_achieved);
    assert_eq!(controller.history().appearances().count(), 0);
    let even = choose(&mut controller, &[map], policy(24.0));
    assert_eq!(even.ticket.intent(), Kind::Structure);
    assert_eq!(
        even.target.unwrap().key.category,
        Category::WeatheredMasonry
    );
}

#[test]
fn displayed_support_credits_chosen_and_incidental_categories_once_per_run() {
    let map = basic(&[
        ([0, 0], Category::OpenGrassland),
        ([-1, 0], Category::WeatheredMasonry),
    ]);
    let mut controller = Controller::new(1, History::default()).unwrap();
    let plan = choose(&mut controller, std::slice::from_ref(&map), policy(24.0));
    let (view, ms) = closest_view(&mut controller, &plan);
    let owners = pixels_for(&map, view, true);
    let result = controller
        .presented(view.tag, &owners, &mut budget())
        .unwrap();
    assert!(result.chosen_visible);
    assert_eq!(result.credited_categories, 2);
    assert_eq!(controller.history().appearances().count(), 2);
    let before = controller.history();
    assert_eq!(
        controller.presented(view.tag, &owners, &mut budget()),
        Err(Error::Stale)
    );
    let next = controller
        .advance(plan.ticket, clock(ms), &budget())
        .unwrap();
    assert_eq!(
        controller
            .presented(next.tag, &owners, &mut budget())
            .unwrap()
            .credited_categories,
        0
    );
    assert_eq!(controller.history(), before);
    let finished = complete(&mut controller, &plan, ms, true);
    assert!(finished.completion.novelty_achieved);
    let even = choose(&mut controller, &[map], policy(24.0));
    assert_eq!(even.choice, Choice::RepeatedAppearance);
    assert_eq!(
        even.fallback,
        Some(Fallback::DesiredAppearancesRecentlySeen)
    );
}

#[test]
fn separate_cached_draws_can_retag_without_movement_after_an_empty_receipt() {
    let map = basic(&[([0, 0], Category::OpenGrassland)]);
    let mut controller = Controller::new(1, History::default()).unwrap();
    let plan = choose(&mut controller, std::slice::from_ref(&map), policy(24.0));
    let (view, ms) = closest_view(&mut controller, &plan);
    controller.presented(view.tag, &[], &mut budget()).unwrap();
    assert_eq!(controller.history(), History::default());
    let owners = pixels_for(&map, view, true);
    assert_eq!(
        controller.presented(view.tag, &owners, &mut budget()),
        Err(Error::Stale)
    );
    let next_draw = controller
        .advance(plan.ticket, clock(ms), &budget())
        .unwrap();
    assert_ne!(next_draw.tag, view.tag);
    assert_eq!(next_draw.look_at, view.look_at);
    assert_eq!(next_draw.eye_y, view.eye_y);
    assert_eq!(next_draw.data_radius, view.data_radius);
    assert_eq!(next_draw.motion, Motion::Frozen);
    let shown = controller
        .presented(next_draw.tag, &owners, &mut budget())
        .unwrap();
    assert!(shown.chosen_visible);
    assert_eq!(shown.credited_categories, 1);
    assert_eq!(controller.history().appearances().count(), 1);
    let repeated = controller
        .advance(plan.ticket, clock(ms), &budget())
        .unwrap();
    assert_eq!(
        controller
            .presented(repeated.tag, &owners, &mut budget())
            .unwrap()
            .credited_categories,
        0
    );
}

#[test]
fn missing_anchor_corroboration_landmark_or_pixel_support_never_earns_credit() {
    for fault in 0..6 {
        let map = basic(&[([0, 0], Category::WeatheredMasonry)]);
        let mut history = History {
            completed: 1,
            ..History::default()
        };
        history.changed().unwrap();
        let mut controller = Controller::new(1, history).unwrap();
        let plan = choose(&mut controller, std::slice::from_ref(&map), policy(16.0));
        let (view, _) = closest_view(&mut controller, &plan);
        let target = plan.target.unwrap();
        let mut owners = pixels_for(&map, view, true);
        match fault {
            0 => owners.retain(|owner| owner.position != target.key.anchor),
            1 => owners.retain(|owner| owner.position != target.corroboration),
            2 => owners.retain(|owner| {
                owner.position != target.landmarks.into_iter().flatten().last().unwrap()
            }),
            3 => owners.iter_mut().for_each(|owner| owner.pixels = 1),
            4 => owners.retain(|owner| {
                owner.position == target.key.anchor
                    || owner.position == target.corroboration
                    || target.landmarks.contains(&Some(owner.position))
            }),
            _ => owners.iter_mut().for_each(|owner| owner.resolved = false),
        }
        assert!(
            !controller
                .presented(view.tag, &owners, &mut budget())
                .unwrap()
                .chosen_visible
        );
        assert_eq!(controller.history(), history);
    }
}

#[test]
fn partial_frames_do_not_union_into_a_fictitious_visible_cluster() {
    let map = basic(&[([0, 0], Category::OpenGrassland)]);
    let mut controller = Controller::new(1, History::default()).unwrap();
    let plan = choose(&mut controller, std::slice::from_ref(&map), policy(16.0));
    let (view, ms) = closest_view(&mut controller, &plan);
    let target = plan.target.unwrap();
    let all = pixels_for(&map, view, true);
    let first: Vec<_> = all
        .iter()
        .copied()
        .filter(|owner| owner.position != target.corroboration)
        .collect();
    let second: Vec<_> = all
        .iter()
        .copied()
        .filter(|owner| owner.position == target.corroboration)
        .collect();
    assert!(
        !controller
            .presented(view.tag, &first, &mut budget())
            .unwrap()
            .chosen_visible
    );
    let next = controller
        .advance(plan.ticket, clock(ms), &budget())
        .unwrap();
    assert!(
        !controller
            .presented(next.tag, &second, &mut budget())
            .unwrap()
            .chosen_visible
    );
    assert_eq!(controller.history().appearances().count(), 0);
}

#[test]
fn other_sites_and_maps_of_a_seen_category_are_not_new_appearances() {
    let map = basic(&[([0, 0], Category::OpenGrassland)]);
    let older_novel = prepared(
        2,
        1,
        1,
        &rectangle([-3, -3], [3, 3]),
        &[([0, 0], Category::DrySandySurface)],
    );
    let newer_repeat = prepared(
        3,
        1,
        999,
        &rectangle([-3, -3], [3, 3]),
        &[([0, 0], Category::OpenGrassland)],
    );
    let mut history = History::default();
    assert!(history.see(&map.targets[0], 1));
    history.changed().unwrap();
    let mut controller = Controller::new(1, history).unwrap();
    let plan = choose(&mut controller, &[newer_repeat, older_novel], policy(16.0));
    assert_eq!(plan.choice, Choice::NovelAppearance);
    assert_eq!(plan.target.unwrap().key.category, Category::DrySandySurface);
    assert_eq!(plan.source().map, MapId([2; 16]));
}

#[test]
fn attempted_map_diversity_never_promotes_repeated_over_global_novelty() {
    let positions = rectangle([-3, -3], [3, 3]);
    let repeated = prepared(1, 1, 999, &positions, &[([0, 0], Category::OpenGrassland)]);
    let novel = prepared(2, 1, 0, &positions, &[([0, 0], Category::DrySandySurface)]);
    let mut history = History::default();
    assert!(history.see(&repeated.targets[0], 1));
    history.changed().unwrap();
    let mut controller = Controller::new(1, history).unwrap();
    let ticket = controller.request(&budget()).unwrap();
    let maps = [Arc::clone(&repeated), Arc::clone(&novel)];
    let attempts = BTreeMap::from([(repeated.source.map, 0), (novel.source.map, 9)]);
    let first = select_diverse_excluding(
        ticket,
        &maps,
        &controller.history(),
        policy(16.0),
        CandidateSurvey {
            excluded: &BTreeSet::new(),
            attempted_maps: &attempts,
            required_choice: None,
        },
        &mut budget(),
    )
    .unwrap()
    .plan
    .unwrap();
    assert_eq!(first.choice(), Choice::NovelAppearance);
    assert_eq!(first.source().map, novel.source.map);
    let repeated_only = select_diverse_excluding(
        ticket,
        &maps,
        &controller.history(),
        policy(16.0),
        CandidateSurvey {
            excluded: &BTreeSet::new(),
            attempted_maps: &attempts,
            required_choice: Some(Choice::RepeatedAppearance),
        },
        &mut budget(),
    )
    .unwrap()
    .plan
    .unwrap();
    assert_eq!(repeated_only.source().map, repeated.source.map);
    assert_eq!(repeated_only.choice(), Choice::RepeatedAppearance);
}

#[test]
fn attempted_map_diversity_only_breaks_ties_within_the_requested_phase() {
    let positions = rectangle([-3, -3], [3, 3]);
    let older = prepared(1, 1, 0, &positions, &[]);
    let newer = prepared(2, 1, 999, &positions, &[]);
    let mut controller = Controller::new(1, History::default()).unwrap();
    let ticket = controller.request(&budget()).unwrap();
    let attempts = BTreeMap::from([(older.source.map, 0), (newer.source.map, 1)]);
    let selected = select_diverse_excluding(
        ticket,
        &[newer, Arc::clone(&older)],
        &controller.history(),
        policy(8.0),
        CandidateSurvey {
            excluded: &BTreeSet::new(),
            attempted_maps: &attempts,
            required_choice: Some(Choice::SavedSurface),
        },
        &mut budget(),
    )
    .unwrap()
    .plan
    .unwrap();
    assert_eq!(selected.source().map, older.source.map);
    assert_eq!(selected.choice(), Choice::SavedSurface);
    assert_eq!(controller.history(), History::default());
}

#[test]
fn route_selection_skips_a_recent_map_when_its_complete_projected_source_is_missing() {
    use super::super::source_footprint;
    use crate::voxel_landscape::assets::budget::{ByteBudget, Cancel};
    use std::sync::atomic::AtomicBool;

    let positions = rectangle([-3, -3], [3, 3]);
    let incomplete = prepared(1, 1, 100, &positions, &[]);
    let complete = prepared(2, 1, 10, &positions, &[]);
    let mut controller = Controller::new(1, History::default()).unwrap();
    let ticket = controller.request(&budget()).unwrap();
    let account = ByteBudget::new(256 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let complete_allocations = rectangle([-32, -32], [32, 32]);
    let incomplete_allocations = BTreeSet::new();
    let mut eligibility =
        |source: Source, line: Line, focus_y: f64, work: &mut Budget<'_>| -> Result<bool, Error> {
            let estimate = source_footprint::work_estimate(line, focus_y, [1, 1], 1024.0).unwrap();
            work.charge(estimate)?;
            let request =
                match source_footprint::request(line, focus_y, [1, 1], 1024.0, &account, cancel) {
                    Ok(request) => request,
                    Err(source_footprint::Error::Invalid | source_footprint::Error::Limit) => {
                        return Ok(false);
                    }
                    Err(error @ source_footprint::Error::Asset(_)) => {
                        return Err(Error::CandidateCoverage(error.to_string()));
                    }
                };
            let allocations = if source.map == MapId([1; 16]) {
                &incomplete_allocations
            } else {
                &complete_allocations
            };
            Ok(request
                .support_chunks()
                .iter()
                .all(|position| allocations.contains(position)))
        };
    let selection = select_diverse_excluding_with_eligibility(
        ticket,
        &[incomplete, Arc::clone(&complete)],
        &controller.history(),
        policy(8.0),
        CandidateSurvey {
            excluded: &BTreeSet::new(),
            attempted_maps: &BTreeMap::new(),
            required_choice: Some(Choice::SavedSurface),
        },
        &mut eligibility,
        &mut budget(),
    )
    .unwrap();
    let selected = selection.plan.unwrap();

    assert_eq!(
        selected.source().map,
        complete.source().map,
        "relative recency must not outrank complete saved source coverage"
    );
    assert!(selection.audit.source_coverage_rejections > 0);
}

#[test]
fn relative_recency_and_displayed_map_route_diversity_are_deterministic() {
    let positions = rectangle([-3, -3], [3, 3]);
    let old = prepared(1, 1, 0, &positions, &[]);
    let newer = prepared(2, 1, 2, &positions, &[]);
    let mut controller = Controller::new(1, History::default()).unwrap();
    let plan = choose(
        &mut controller,
        &[Arc::clone(&old), Arc::clone(&newer)],
        policy(8.0),
    );
    assert_eq!(plan.source().map, newer.source.map);
    let ticket = plan.ticket;
    let reversed = select(
        ticket,
        &[Arc::clone(&newer), Arc::clone(&old)],
        &controller.history(),
        policy(8.0),
        &mut budget(),
    )
    .unwrap()
    .plan
    .unwrap();
    assert_eq!(reversed.route, plan.route);
    controller.start(&plan, clock(0), &budget()).unwrap();
    complete(&mut controller, &plan, 0, true);
    let second = choose(&mut controller, &[old, newer], policy(8.0));
    assert_eq!(second.source().map, MapId([1; 16]));
}

#[test]
fn single_map_completion_selects_a_different_geometry_not_just_a_reversal() {
    let map = basic(&[]);
    let mut controller = Controller::new(1, History::default()).unwrap();
    let first = choose(&mut controller, std::slice::from_ref(&map), policy(8.0));
    controller.start(&first, clock(0), &budget()).unwrap();
    let old_pose = complete(&mut controller, &first, 0, true);
    assert_eq!(old_pose.retired.route, first.route);
    let second = choose(&mut controller, &[map], policy(8.0));
    assert_ne!(first.route, second.route);
    assert_eq!(
        route_key(
            first.source().map,
            Line {
                start: first.line.end,
                end: first.line.start
            }
        ),
        first.route
    );
    assert_sweep(&second);
}

#[test]
fn holey_disconnected_and_negative_coverage_pass_a_continuous_sweep_oracle() {
    let mut holey = rectangle([-4, -4], [4, 4]);
    holey.remove(&[0, 0]);
    holey.remove(&[-1, 1]);
    let mut disconnected = rectangle([-8, -2], [-4, 2]);
    disconnected.extend(rectangle([4, -2], [8, 2]));
    for positions in [holey, disconnected] {
        let map = prepared(1, 1, 1, &positions, &[]);
        for radius in [0.0, 8.0, 16.0] {
            let mut controller = Controller::new(1, History::default()).unwrap();
            let plan = choose(&mut controller, std::slice::from_ref(&map), policy(radius));
            assert_sweep(&plan);
            for step in 0..=1000 {
                assert!(map
                    .loaded
                    .coverage
                    .contains_view(plan.line.point(f64::from(step) / 1000.0), radius));
            }
        }
    }
}

#[test]
fn narrow_strips_and_missing_view_halo_never_get_a_fake_long_route() {
    let positions = rectangle([-5, 0], [5, 0]);
    let map = prepared(1, 1, 1, &positions, &[]);
    let mut controller = Controller::new(1, History::default()).unwrap();
    let ticket = controller.request(&budget()).unwrap();
    let narrow = select(
        ticket,
        std::slice::from_ref(&map),
        &controller.history(),
        policy(8.0),
        &mut budget(),
    )
    .unwrap();
    assert!(narrow.plan.is_none());
    let mut fitting = policy(6.0);
    fitting.envelope.horizontal_halo = 1.0;
    let fit = select(
        ticket,
        std::slice::from_ref(&map),
        &controller.history(),
        fitting,
        &mut budget(),
    )
    .unwrap()
    .plan
    .unwrap();
    assert_sweep(&fit);
    fitting.envelope.horizontal_halo = 2.0;
    assert!(select(
        ticket,
        &[map],
        &controller.history(),
        fitting,
        &mut budget()
    )
    .unwrap()
    .plan
    .is_none());
}

#[test]
fn proto_incomplete_unaddressable_and_mismatched_coverage_are_rejected() {
    for fault in 0..6 {
        let mut chunk = decoded([0, 0], None, 3218);
        match fault {
            0 => chunk.status = Some("features".into()),
            1 => {
                chunk.sections.remove(&0);
            }
            2 => chunk.sections_present = false,
            3 => chunk.identity.data_version = 2833,
            4 => chunk.identity.position = [1, 0],
            _ => {
                chunk.sections.insert(20, chunk.sections[&0].clone());
            }
        }
        let mut loaded = LoadedWindow::default();
        loaded.chunks.insert([0, 0], Arc::new(chunk));
        loaded.coverage.chunks.insert([0, 0]);
        assert!(matches!(
            PreparedMap::new(source(1, 1), 0, Arc::new(loaded), vec![], &mut budget()),
            Err(Error::Invalid(_))
        ));
    }
    let mut loaded = loaded(&rectangle([0, 0], [0, 0]));
    loaded.coverage.chunks.clear();
    assert!(PreparedMap::new(source(1, 1), 0, Arc::new(loaded), vec![], &mut budget()).is_err());
}

#[test]
fn rejected_regions_outside_qualified_coverage_do_not_discard_safe_islands() {
    let mut data = loaded(&rectangle([-3, -3], [3, 3]));
    data.rejected_chunks = 17;
    data.issues.push(super::super::loader::Issue {
        position: [90, 0],
        reason: super::super::loader::Rejection::Absent,
    });
    let map =
        Arc::new(PreparedMap::new(source(1, 1), 0, Arc::new(data), vec![], &mut budget()).unwrap());
    let mut controller = Controller::new(1, History::default()).unwrap();
    assert_sweep(&choose(&mut controller, &[map], policy(16.0)));
}

#[test]
fn saved_source_accepts_three_window_footprint_and_clones_retain_projected_charge() {
    use crate::voxel_landscape::assets::budget::{ByteBudget, Cancel};
    use std::sync::atomic::AtomicBool;

    let original = PreparedMap::new(
        source(4, 7),
        0,
        Arc::new(loaded(&rectangle([0, 0], [0, 0]))),
        vec![],
        &mut budget(),
    )
    .unwrap();
    let expanded = Arc::new(loaded(&rectangle([0, 0], [128, 0])));
    let saved = PreparedMap::new(
        source(4, 7),
        0,
        Arc::clone(&expanded),
        vec![],
        &mut budget(),
    )
    .unwrap();
    assert_eq!(saved.loaded().chunks.len(), 129);

    let template = Arc::clone(expanded.chunks.get(&[0, 0]).unwrap());
    let mut oversized = super::super::loader::LoadedWindow::default();
    for x in 0..=384 {
        let position = [x, 0];
        oversized.chunks.insert(position, Arc::clone(&template));
        oversized.coverage.chunks.insert(position);
    }
    assert!(matches!(
        PreparedMap::new(source(4, 7), 0, Arc::new(oversized), vec![], &mut budget()),
        Err(Error::Limit("prepared snapshot"))
    ));
    let account = ByteBudget::new(1 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let charge = account.reserve(4096, Cancel::new(&stop)).unwrap();
    let projected = Arc::new(
        original
            .projected_source(expanded, charge, &mut budget())
            .unwrap(),
    );
    assert_eq!(projected.source(), original.source());
    assert!(projected.targets().is_empty());
    assert_eq!(projected.loaded().chunks.len(), 129);
    let owner_receipt = Arc::clone(&projected);
    drop(projected);
    assert_eq!(account.used(), 4096);
    drop(owner_receipt);
    assert_eq!(account.used(), 0);
}

#[test]
fn unqualified_route_exclusion_reselects_without_writing_history() {
    let map = basic(&[([0, 0], Category::OpenGrassland)]);
    let mut controller = Controller::new(1, History::default()).unwrap();
    let ticket = controller.request(&budget()).unwrap();
    let initial = select(
        ticket,
        std::slice::from_ref(&map),
        &controller.history(),
        policy(8.0),
        &mut budget(),
    )
    .unwrap()
    .plan
    .unwrap();
    let excluded = BTreeSet::from([initial.route()]);
    let retry = select_excluding(
        ticket,
        &[map],
        &controller.history(),
        policy(8.0),
        &excluded,
        &mut budget(),
    )
    .unwrap();
    assert_ne!(retry.plan.unwrap().route(), initial.route());
    assert_eq!(controller.history(), History::default());
}

#[test]
fn epoch_serial_source_and_history_staleness_fail_without_credit() {
    let map = basic(&[([0, 0], Category::OpenGrassland)]);
    let mut controller = Controller::new(1, History::default()).unwrap();
    let old = choose(&mut controller, std::slice::from_ref(&map), policy(16.0));
    controller.request(&budget()).unwrap();
    assert!(matches!(
        controller.start(&old, clock(0), &budget()),
        Err(Error::Stale)
    ));
    let plan = choose(&mut controller, std::slice::from_ref(&map), policy(16.0));
    let (view, _) = closest_view(&mut controller, &plan);
    let retired = controller.invalidate(2).unwrap().unwrap();
    assert_eq!(retired.ticket, plan.ticket);
    assert_eq!(
        controller.presented(view.tag, &[], &mut budget()),
        Err(Error::Idle)
    );
    assert!(controller.view().is_none());
    assert_eq!(controller.history(), History::default());
    let ticket = controller.request(&budget()).unwrap();
    assert!(matches!(
        select(
            ticket,
            &[map],
            &controller.history(),
            policy(16.0),
            &mut budget()
        ),
        Err(Error::Stale)
    ));
    assert!(matches!(controller.invalidate(1), Err(Error::Stale)));
    controller.invalidate(3).unwrap(); // Returning to an old folder still needs a fresh epoch.
    assert!(matches!(
        controller.start(&plan, clock(0), &budget()),
        Err(Error::Stale)
    ));
}

#[test]
fn forged_state_duplicate_owner_wrong_map_and_stale_frame_are_rejected_atomically() {
    let map = basic(&[([0, 0], Category::OpenGrassland)]);
    let other = prepared(
        2,
        1,
        0,
        &rectangle([-3, -3], [3, 3]),
        &[([0, 0], Category::OpenGrassland)],
    );
    let mut controller = Controller::new(1, History::default()).unwrap();
    let plan = choose(&mut controller, std::slice::from_ref(&map), policy(16.0));
    let (view, ms) = closest_view(&mut controller, &plan);
    let owners = pixels_for(&map, view, true);
    let oversized = vec![owners[0]; MAX_OWNERS + 1];
    assert_eq!(
        controller.presented(view.tag, &oversized, &mut budget()),
        Err(Error::Limit("display owners"))
    );
    let mut duplicate = owners.clone();
    duplicate.insert(0, owners[0]);
    assert!(matches!(
        controller.presented(view.tag, &duplicate, &mut budget()),
        Err(Error::Invalid(_))
    ));
    let mut foreign = owners.clone();
    foreign[0].state = other.state(foreign[0].position).unwrap();
    assert!(matches!(
        controller.presented(view.tag, &foreign, &mut budget()),
        Err(Error::Invalid(_))
    ));
    let mut outside = owners.clone();
    outside[0].position = [10_000, 64, 10_000];
    outside.sort_unstable_by_key(display_order);
    assert!(matches!(
        controller.presented(view.tag, &outside, &mut budget()),
        Err(Error::Invalid(_))
    ));
    assert_eq!(controller.history(), History::default());
    let new_view = controller
        .advance(plan.ticket, clock(ms), &budget())
        .unwrap();
    assert_eq!(
        controller.presented(view.tag, &owners, &mut budget()),
        Err(Error::Stale)
    );
    assert!(
        controller
            .presented(new_view.tag, &owners, &mut budget())
            .unwrap()
            .chosen_visible
    );
}

#[test]
fn freeze_resume_speed_changes_and_clock_jumps_do_not_skip_or_rewind_routes() {
    let map = basic(&[]);
    let mut controller = Controller::new(1, History::default()).unwrap();
    let plan = choose(&mut controller, &[map], policy(8.0));
    assert_eq!(
        controller
            .start(
                &plan,
                Clock {
                    local_speed: 0.0,
                    ..clock(0)
                },
                &budget()
            )
            .unwrap(),
        None
    );
    assert_eq!(
        controller
            .start(
                &plan,
                Clock {
                    frozen: true,
                    ..clock(0)
                },
                &budget()
            )
            .unwrap(),
        None
    );
    let initial = controller
        .start(&plan, clock(0), &budget())
        .unwrap()
        .unwrap();
    let held = controller
        .advance(
            plan.ticket,
            Clock {
                local_speed: 0.0,
                ..clock(250)
            },
            &budget(),
        )
        .unwrap();
    assert_eq!(held.look_at, initial.look_at);
    let resumed = controller
        .advance(plan.ticket, clock(500), &budget())
        .unwrap();
    assert_eq!(resumed.look_at, initial.look_at); // The preceding zero-speed interval stays frozen.
    let moved = controller
        .advance(
            plan.ticket,
            Clock {
                local_speed: 32.0,
                ..clock(750)
            },
            &budget(),
        )
        .unwrap();
    assert!((controller.active.as_ref().unwrap().distance - 16.0).abs() < 1e-9);
    controller
        .advance(plan.ticket, clock(1000), &budget())
        .unwrap();
    assert!((controller.active.as_ref().unwrap().distance - 24.0).abs() < 1e-9);
    let previous = controller.view().unwrap();
    let held = controller
        .advance(plan.ticket, clock(1000), &budget())
        .unwrap();
    assert_eq!(held.look_at, previous.look_at);
    let jump = controller
        .advance(plan.ticket, clock(100_000), &budget())
        .unwrap();
    assert_eq!(jump.motion, Motion::RebasedForwardJump);
    assert_eq!(jump.look_at, held.look_at);
    assert!(matches!(
        controller.advance(plan.ticket, clock(1), &budget()),
        Err(Error::ClockReversed)
    ));
    assert_eq!(controller.view(), Some(jump));
    assert_ne!(moved.look_at, initial.look_at);
    assert_eq!(controller.history().completed(), 0);
}

#[test]
fn endpoint_needs_exact_presentation_and_frozen_transition_cannot_finish() {
    let map = basic(&[]);
    let mut controller = Controller::new(1, History::default()).unwrap();
    let plan = choose(&mut controller, &[map], policy(8.0));
    controller.start(&plan, clock(0), &budget()).unwrap();
    assert!(matches!(
        controller.finish(plan.ticket, clock(0), &budget()),
        Err(Error::NotComplete)
    ));
    for step in 1..=256 {
        let ms = step * 250;
        let view = controller
            .advance(plan.ticket, clock(ms), &budget())
            .unwrap();
        if view.motion != Motion::Endpoint {
            continue;
        }
        assert!(matches!(
            controller.finish(plan.ticket, clock(ms), &budget()),
            Err(Error::NotComplete)
        ));
        controller.presented(view.tag, &[], &mut budget()).unwrap();
        assert!(matches!(
            controller.finish(
                plan.ticket,
                Clock {
                    frozen: true,
                    ..clock(ms)
                },
                &budget()
            ),
            Err(Error::Frozen)
        ));
        let result = controller
            .finish(plan.ticket, clock(ms), &budget())
            .unwrap();
        assert_eq!(result.completion.run, 1);
        assert!(!result.completion.novelty_achieved);
        assert!(matches!(
            controller.finish(plan.ticket, clock(ms), &budget()),
            Err(Error::Idle)
        ));
        assert_eq!(controller.history().completed(), 1);
        return;
    }
    panic!("bounded fixture did not reach endpoint");
}

#[test]
fn bounded_history_round_trips_and_category_recency_expires_by_completed_runs() {
    let map = basic(&[([0, 0], Category::OpenGrassland)]);
    let mut history = History::default();
    history.see(&map.targets[0], 1);
    history.completed = 1;
    history.changed().unwrap();
    assert!(history.recent(Category::OpenGrassland, 9));
    assert!(!history.recent(Category::OpenGrassland, 10));
    let json = serde_json::to_string(&history).unwrap();
    let restored: History = serde_json::from_str(&json).unwrap();
    assert_eq!(restored, history);
    assert_eq!(
        restored.appearances().next().unwrap().key,
        map.targets[0].key
    );
    Controller::new(2, restored).unwrap();
    let mut corrupt = history;
    corrupt.seen[1] = corrupt.seen[0];
    assert!(Controller::new(2, corrupt).is_err());
    corrupt = history;
    corrupt.seen[0].as_mut().unwrap().run = 99;
    assert!(Controller::new(2, corrupt).is_err());
    for run in 1..=100 {
        history.routes.rotate_right(1);
        history.routes[0] = Some(Traversal {
            route: RouteKey {
                map: MapId([1; 16]),
                endpoints: [[run as i64, 0], [run as i64 + 200, 0]],
            },
            run,
        });
        history.completed = run;
    }
    assert_eq!(history.traversals().count(), 16);
    assert_eq!(history.appearances().count(), 1);
}

#[test]
fn finite_query_limit_and_absent_kind_fallbacks_are_explicit_not_novelty_success() {
    let map = basic(&[([0, 0], Category::OpenGrassland)]);
    let mut controller = Controller::new(1, History::default()).unwrap();
    let ticket = controller.request(&budget()).unwrap();
    let limited = select(
        ticket,
        std::slice::from_ref(&map),
        &controller.history(),
        Policy {
            max_line_queries: 1,
            ..policy(8.0)
        },
        &mut budget(),
    )
    .unwrap();
    assert_eq!(limited.audit.line_queries, 1);
    assert!(limited.audit.query_limited);
    assert_eq!(limited.plan.unwrap().choice, Choice::NovelAppearance);
    assert_eq!(controller.history(), History::default());
    let history = History {
        completed: 1,
        ..History::default()
    };
    let mut even = Controller::new(1, history).unwrap();
    let plan = choose(&mut even, &[map], policy(8.0));
    assert_eq!(plan.choice, Choice::OtherKind);
    assert_eq!(
        plan.fallback,
        Some(Fallback::DesiredKindAbsentFromSuppliedEvidence)
    );
    let (view, ms) = closest_view(&mut even, &plan);
    let owners = pixels_for(&plan.map, view, true);
    even.presented(view.tag, &owners, &mut budget()).unwrap();
    assert!(
        !complete(&mut even, &plan, ms, true)
            .completion
            .novelty_achieved
    );
}

#[test]
fn no_maps_empty_coverage_and_zero_surface_seeds_remain_unavailable() {
    let mut controller = Controller::new(1, History::default()).unwrap();
    let ticket = controller.request(&budget()).unwrap();
    let empty = select(
        ticket,
        &[],
        &controller.history(),
        policy(8.0),
        &mut budget(),
    )
    .unwrap();
    assert_eq!(empty.unavailable, Some(Unavailable::NoPreparedMaps));
    let empty_map = Arc::new(
        PreparedMap::new(
            source(1, 1),
            0,
            Arc::new(LoadedWindow::default()),
            vec![],
            &mut budget(),
        )
        .unwrap(),
    );
    assert_eq!(
        select(
            ticket,
            &[empty_map],
            &controller.history(),
            policy(8.0),
            &mut budget()
        )
        .unwrap()
        .unavailable,
        Some(Unavailable::NoQualifiedCoverage)
    );
    let mut data = loaded(&rectangle([-3, -3], [3, 3]));
    for chunk in data.chunks.values_mut() {
        let chunk = Arc::get_mut(chunk).unwrap();
        let air = chunk.sections[&19].block_states.clone();
        for section in chunk.sections.values_mut() {
            section.block_states = air.clone();
        }
    }
    let void =
        Arc::new(PreparedMap::new(source(1, 1), 0, Arc::new(data), vec![], &mut budget()).unwrap());
    let result = select(
        ticket,
        &[void],
        &controller.history(),
        policy(8.0),
        &mut budget(),
    )
    .unwrap();
    assert!(result.plan.is_none());
    assert_eq!(result.audit.surface_seeds, 0);
    assert_eq!(controller.history(), History::default());
}

#[test]
fn malformed_summary_and_resource_policy_bounds_fail_closed() {
    let map = basic(&[([0, 0], Category::OpenGrassland)]);
    for fault in 0..4 {
        let mut target = map.targets[0];
        match fault {
            0 => target.source.generation = 2,
            1 => target.support.columns = 1,
            2 => target.key.anchor[1] = 320,
            _ => target.support.maximum[0] = i32::MAX,
        }
        assert!(PreparedMap::new(
            map.source,
            0,
            Arc::clone(&map.loaded),
            vec![target],
            &mut budget()
        )
        .is_err());
    }
    let mut controller = Controller::new(1, History::default()).unwrap();
    let ticket = controller.request(&budget()).unwrap();
    for fault in 0..6 {
        let mut policy = policy(8.0);
        match fault {
            0 => policy.minimum_length = 0.0,
            1 => policy.maximum_length = f64::INFINITY,
            2 => policy.envelope.viewport_radius = f64::NAN,
            3 => policy.envelope.eye_y = 320.0,
            4 => policy.envelope.horizontal_halo = 129.0,
            _ => policy.max_line_queries = 0,
        }
        assert!(matches!(
            select(
                ticket,
                std::slice::from_ref(&map),
                &controller.history(),
                policy,
                &mut budget()
            ),
            Err(Error::Invalid(_))
        ));
    }
    assert!(matches!(
        select(
            ticket,
            &vec![Arc::clone(&map); 17],
            &controller.history(),
            policy(8.0),
            &mut budget()
        ),
        Err(Error::Limit("maps"))
    ));
    assert!(matches!(
        select(
            ticket,
            &[Arc::clone(&map), map],
            &controller.history(),
            policy(8.0),
            &mut budget()
        ),
        Err(Error::Invalid(_))
    ));
}

#[test]
fn cancellation_and_work_exhaustion_are_transactional_in_search_and_credit() {
    let map = basic(&[([0, 0], Category::OpenGrassland)]);
    let mut controller = Controller::new(1, History::default()).unwrap();
    let ticket = controller.request(&budget()).unwrap();
    assert!(matches!(
        select(
            ticket,
            std::slice::from_ref(&map),
            &controller.history(),
            policy(16.0),
            &mut Budget::new(0, &|| false)
        ),
        Err(Error::Limit("work"))
    ));
    let calls = Cell::new(0);
    let counting = || {
        calls.set(calls.get() + 1);
        false
    };
    let plan = select(
        ticket,
        std::slice::from_ref(&map),
        &controller.history(),
        policy(16.0),
        &mut Budget::new(u64::MAX, &counting),
    )
    .unwrap()
    .plan
    .unwrap();
    let total = calls.get();
    for stop in [1, 10, total] {
        calls.set(0);
        let cancel = || {
            calls.set(calls.get() + 1);
            calls.get() == stop
        };
        assert!(matches!(
            select(
                ticket,
                std::slice::from_ref(&map),
                &controller.history(),
                policy(16.0),
                &mut Budget::new(u64::MAX, &cancel)
            ),
            Err(Error::Cancelled)
        ));
    }
    let (view, ms) = closest_view(&mut controller, &plan);
    let owners = pixels_for(&map, view, true);
    assert!(matches!(
        controller.presented(
            view.tag,
            &owners,
            &mut Budget::new(owners.len() as u64 + 1, &|| false)
        ),
        Err(Error::Limit("work"))
    ));
    assert_eq!(controller.history(), History::default());
    let calls = Cell::new(0);
    let cancel = || {
        calls.set(calls.get() + 1);
        calls.get() == owners.len() + 5
    };
    assert!(matches!(
        controller.presented(view.tag, &owners, &mut Budget::new(u64::MAX, &cancel)),
        Err(Error::Cancelled)
    ));
    assert_eq!(controller.history(), History::default());
    assert_eq!(controller.active.as_ref().unwrap().acknowledged, 0);
    assert!(
        controller
            .presented(view.tag, &owners, &mut budget())
            .unwrap()
            .chosen_visible
    );
    let before = controller.view();
    assert!(matches!(
        controller.advance(plan.ticket, clock(ms + 50), &Budget::new(100, &|| true)),
        Err(Error::Cancelled)
    ));
    assert_eq!(controller.view(), before);
}

#[test]
fn signed_coordinate_edges_and_last_supported_run_do_not_wrap() {
    for center in [i32::MIN.div_euclid(16) + 3, i32::MAX.div_euclid(16) - 3] {
        let positions = rectangle([center - 3, -3], [center + 3, 3]);
        let map = prepared(1, 1, 0, &positions, &[]);
        let history = History {
            completed: u64::MAX - 1,
            ..History::default()
        };
        let mut controller = Controller::new(1, history).unwrap();
        let plan = choose(&mut controller, &[map], policy(8.0));
        assert_eq!(plan.ticket.run, u64::MAX);
        assert_sweep(&plan);
    }
}

#[test]
fn confidence_floor_applies_to_selection_and_to_displayed_evidence() {
    let strong = basic(&[
        ([0, 0], Category::OpenGrassland),
        ([-1, 0], Category::WeatheredMasonry),
    ]);
    let mut targets = strong.targets.clone();
    // A deliberately conservative producer assessment may be weaker than support.
    for target in &mut targets {
        if target.key.category == Category::WeatheredMasonry {
            target.confidence = Confidence::Supported;
        }
    }
    let map = Arc::new(
        PreparedMap::new(
            strong.source,
            0,
            Arc::clone(&strong.loaded),
            targets,
            &mut budget(),
        )
        .unwrap(),
    );
    let history = History {
        completed: 1,
        ..History::default()
    };
    let mut controller = Controller::new(1, history).unwrap();
    let policy = Policy {
        minimum_confidence: Confidence::Corroborated,
        ..policy(24.0)
    };
    let plan = choose(&mut controller, std::slice::from_ref(&map), policy);
    assert_eq!(plan.choice, Choice::OtherKind);
    assert_eq!(plan.fallback, Some(Fallback::DesiredKindBelowConfidence));
    let (view, _) = closest_view(&mut controller, &plan);
    let owners = pixels_for(&map, view, true);
    let weak = map
        .targets
        .iter()
        .find(|target| target.key.category == Category::WeatheredMasonry)
        .unwrap();
    assert!(visible(weak, &owners, 2, &mut budget()).unwrap());
    let result = controller
        .presented(view.tag, &owners, &mut budget())
        .unwrap();
    assert!(result.chosen_visible);
    assert_eq!(result.credited_categories, 1);
    assert!(!controller
        .history()
        .appearances()
        .any(|seen| seen.key.category == Category::WeatheredMasonry));
}

#[test]
fn shifted_support_requires_final_owners_on_both_sides_of_chunk_seams() {
    let positions = rectangle([0, 0], [1, 1]);
    let mut data = loaded(&positions);
    for (&position, chunk) in &mut data.chunks {
        *chunk = Arc::new(decoded(
            position,
            Some(Category::DwellingLikeConstruction),
            3218,
        ));
    }
    let loaded = Arc::new(data);
    let mut footprint = [0_u64; 4];
    for z in 14..=17 {
        for x in 14..=17 {
            let index = ((z - 8) * 16 + x - 8) as usize;
            footprint[index / 64] |= 1_u64 << (index % 64);
        }
    }
    // This isolates the tour support contract. The classifier's semantic
    // pairing and connectedness are covered by evidence_tests instead.
    let target = TargetSummary {
        source: source(1, 1),
        key: TargetKey {
            map: source(1, 1).map,
            revision: evidence::RULE_REVISION,
            category: Category::DwellingLikeConstruction,
            tile: [0, 0],
            anchor: [15, 63, 15],
        },
        confidence: Confidence::Supported,
        support: evidence::Support {
            columns: 16,
            primary_columns: 14,
            secondary_columns: 2,
            secondary_sectors: 4,
            links: 1,
            minimum: [14, 63, 14],
            maximum: [17, 63, 17],
            origin: [8, 8],
            footprint,
        },
        corroboration: [16, 63, 15],
        landmarks: [Some([14, 63, 14]), Some([17, 63, 17]), None, None],
        anchor_only_air_above: false,
        anchor_water_above: false,
    };
    let map = PreparedMap::new(source(1, 1), 100, loaded, vec![target], &mut budget()).unwrap();
    let target = &map.targets()[0];
    let mut owners = Vec::new();
    for z in 14..=17 {
        for x in 14..=17 {
            let position = [x, 63, z];
            owners.push(DisplayedBlock {
                position,
                state: map.state(position).unwrap(),
                pixels: 2,
                resolved: true,
            });
        }
    }
    owners.sort_unstable_by_key(display_order);
    assert!(visible(target, &owners, 2, &mut budget()).unwrap());
    owners.retain(|owner| owner.position != target.corroboration);
    // A hidden identification cell does not erase sufficient visible exterior.
    assert!(visible(target, &owners, 2, &mut budget()).unwrap());
    owners.retain(|owner| owner.position[0] < 16);
    assert!(!visible(target, &owners, 2, &mut budget()).unwrap());
}

#[test]
fn viewport_cancellation_keeps_display_credit_but_never_finishes_partial_route() {
    let map = basic(&[([0, 0], Category::OpenGrassland)]);
    let mut controller = Controller::new(1, History::default()).unwrap();
    let plan = choose(&mut controller, std::slice::from_ref(&map), policy(24.0));
    let (view, _) = closest_view(&mut controller, &plan);
    let owners = pixels_for(&map, view, true);
    assert_eq!(
        controller
            .presented(view.tag, &owners, &mut budget())
            .unwrap()
            .credited_categories,
        1
    );
    let credited = controller.history();
    assert_eq!(credited.appearances().count(), 1);
    assert_eq!(credited.completed(), 0);
    let retired = controller.cancel_viewport(&budget()).unwrap().unwrap();
    assert_eq!(retired.ticket(), plan.ticket());
    assert_eq!(controller.history(), credited);
    assert!(controller.view().is_none());
    assert!(matches!(
        controller.presented(view.tag, &[], &mut budget()),
        Err(Error::Stale)
    ));
    let next = controller.request(&budget()).unwrap();
    assert_eq!(next.generation(), view.tag.ticket.generation());
    assert_eq!(next.run(), plan.ticket().run());
    assert!(next.serial > plan.ticket().serial);
}

#[test]
fn pending_only_viewport_cancellation_preserves_history_and_increases_serial() {
    let mut controller = Controller::new(1, History::default()).unwrap();
    let previous = controller.request(&budget()).unwrap();
    assert!(controller.cancel_viewport(&budget()).unwrap().is_none());
    assert_eq!(controller.history(), History::default());
    let next = controller.request(&budget()).unwrap();
    assert_eq!(next.generation(), previous.generation());
    assert_eq!(next.run(), previous.run());
    assert!(next.serial > previous.serial);
}

#[test]
fn projected_pixels_credit_original_target_using_redecoded_palette_owners() {
    use crate::voxel_landscape::assets::budget::{ByteBudget, Cancel};
    use std::sync::atomic::AtomicBool;
    let base = basic(&[([0, 0], Category::OpenGrassland)]);
    let mut controller = Controller::new(1, History::default()).unwrap();
    let plan = choose(&mut controller, std::slice::from_ref(&base), policy(24.0));
    let account = ByteBudget::new(256 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let size = [64, 64];
    let scale = 4.0;
    let request = Arc::new(
        super::super::source_footprint::request(
            plan.line(),
            plan.focus_y(),
            size,
            scale,
            &account,
            cancel,
        )
        .unwrap(),
    );
    let mut decoded_source = loaded(request.support_chunks());
    for (&position, chunk) in &base.loaded().chunks {
        if decoded_source.chunks.contains_key(&position) {
            decoded_source
                .chunks
                .insert(position, Arc::new((**chunk).clone()));
        }
    }
    let map = Arc::new(
        base.projected_source(
            Arc::new(decoded_source),
            account.reserve(1, cancel).unwrap(),
            &mut budget(),
        )
        .unwrap(),
    );
    assert!(map.targets().is_empty());
    let display = Arc::new(
        ProjectedDisplay::bind(
            &plan,
            Arc::clone(&map),
            Arc::clone(&request),
            size,
            scale,
            &mut budget(),
        )
        .unwrap(),
    );
    let mut view = controller
        .start_projected(&plan, display, clock(0), &budget())
        .unwrap()
        .unwrap();
    let anchor = plan.target.unwrap().key.anchor;
    for step in 1..=512 {
        if (view.look_at[0] - (f64::from(anchor[0]) + 0.5))
            .hypot(view.look_at[2] - (f64::from(anchor[2]) + 0.5))
            < 1.7
        {
            break;
        }
        view = controller
            .advance(plan.ticket(), clock(step * 50), &budget())
            .unwrap();
    }
    let original_owners = pixels_for(&base, view, true);
    let owners: Vec<_> = original_owners
        .iter()
        .filter(|owner| {
            request.may_project_cell(owner.position, [view.look_at[0], view.look_at[2]])
        })
        .map(|owner| {
            let state = map.state(owner.position).unwrap();
            assert_eq!(state, owner.state);
            assert!(
                !std::ptr::eq(state, owner.state),
                "fixture must use redecoded palette storage"
            );
            DisplayedBlock {
                position: owner.position,
                state,
                pixels: owner.pixels,
                resolved: true,
            }
        })
        .collect();
    assert!(!owners.is_empty());
    let issued = controller.issued_view().unwrap();
    let presentation = controller
        .presented_issued(&issued, &owners, &mut budget())
        .unwrap();
    assert!(presentation.chosen_visible);
    assert_eq!(presentation.credited_categories, 1);
    assert_eq!(controller.history().appearances().count(), 1);
    assert_eq!(controller.history().completed(), 0);
    assert_eq!(
        controller
            .presented_issued(&issued, &owners, &mut budget())
            .unwrap()
            .credited_categories,
        0
    );
}

// Synthetic exterior owner samples shaped like the captured216 dwelling.
// The eight-column positive adds one visible column; captured seven stays negative.
fn roofed_dwelling_samples() -> (TargetSummary, [BlockState; 3], Vec<[i32; 3]>) {
    let mut footprint = [0_u64; 4];
    for z in -18..=-14 {
        for x in 222..=226 {
            let index = ((z + 24) * 16 + x - 216) as usize;
            footprint[index / 64] |= 1_u64 << (index % 64);
        }
    }
    let target = TargetSummary {
        source: source(1, 1),
        key: TargetKey {
            map: source(1, 1).map,
            revision: evidence::RULE_REVISION,
            category: Category::DwellingLikeConstruction,
            tile: [13, -1],
            anchor: [223, 81, -16],
        },
        confidence: Confidence::Corroborated,
        support: evidence::Support {
            columns: 25,
            primary_columns: 25,
            secondary_columns: 2,
            secondary_sectors: 4,
            links: 2,
            minimum: [222, 80, -18],
            maximum: [226, 85, -14],
            origin: [216, -24],
            footprint,
        },
        corroboration: [223, 81, -17],
        landmarks: [
            Some([222, 83, -18]),
            Some([226, 83, -18]),
            Some([222, 83, -14]),
            Some([226, 83, -14]),
        ],
        anchor_only_air_above: false,
        anchor_water_above: false,
    };
    let states = [
        BlockState {
            name: "minecraft:spruce_log".into(),
            properties: BTreeMap::from([("axis".into(), "z".into())]),
        },
        BlockState {
            name: "minecraft:cobblestone".into(),
            properties: BTreeMap::new(),
        },
        BlockState {
            name: "minecraft:grass_block".into(),
            properties: BTreeMap::from([("snowy".into(), "false".into())]),
        },
    ];
    let positions = vec![
        [223, 81, -14],
        [225, 85, -18],
        [225, 85, -17],
        [226, 84, -18],
        [225, 85, -16],
        [225, 85, -15],
        [225, 85, -14],
        [224, 85, -18],
    ];
    (target, states, positions)
}

#[test]
fn roofed_dwelling_credits_sufficient_actual_exterior_without_hidden_bed_or_corners() {
    let (target, states, positions) = roofed_dwelling_samples();
    let mut owners: Vec<_> = positions
        .into_iter()
        .enumerate()
        .map(|(i, position)| DisplayedBlock {
            position,
            state: &states[usize::from(i == 0)],
            pixels: 2,
            resolved: true,
        })
        .collect();
    owners.sort_unstable_by_key(display_order);
    assert!(visible(&target, &owners, 1, &mut budget()).unwrap());
    assert!(visible(&target, &owners, 2, &mut budget()).unwrap());
}

#[test]
fn roofed_dwelling_rejects_captured_seven_columns_roof_alone_and_unrelated_terrain() {
    let (target, states, positions) = roofed_dwelling_samples();
    let mut owners: Vec<_> = positions
        .into_iter()
        .enumerate()
        .map(|(i, position)| DisplayedBlock {
            position,
            state: &states[usize::from(i == 0)],
            pixels: 2,
            resolved: true,
        })
        .collect();
    owners.sort_unstable_by_key(display_order);
    let mut seven = owners.clone();
    seven.retain(|owner| owner.position != [224, 85, -18]);
    assert!(!visible(&target, &seven, 1, &mut budget()).unwrap());
    let mut roof = owners.clone();
    for owner in &mut roof {
        owner.state = &states[0];
    }
    assert!(!visible(&target, &roof, 1, &mut budget()).unwrap());
    let mut natural = owners.clone();
    for owner in &mut natural {
        owner.state = &states[2];
    }
    assert!(!visible(&target, &natural, 1, &mut budget()).unwrap());
    let mut unresolved = owners.clone();
    unresolved[0].resolved = false;
    assert!(!visible(&target, &unresolved, 1, &mut budget()).unwrap());
    let mut below_minimum = owners;
    below_minimum[0].pixels = 1;
    assert!(!visible(&target, &below_minimum, 2, &mut budget()).unwrap());
}

#[test]
fn roofed_dwelling_rejects_fluid_only_wet_fence_and_malformed_timber_owners() {
    let (target, states, positions) = roofed_dwelling_samples();
    let wet_fence = BlockState {
        name: "minecraft:oak_fence".into(),
        properties: BTreeMap::from([
            ("east".into(), "false".into()),
            ("north".into(), "false".into()),
            ("south".into(), "false".into()),
            ("west".into(), "false".into()),
            ("waterlogged".into(), "true".into()),
        ]),
    };
    let invalid_timber = BlockState {
        name: "minecraft:spruce_log".into(),
        properties: BTreeMap::from([("axis".into(), "invalid".into())]),
    };
    let mut owners: Vec<_> = positions
        .into_iter()
        .enumerate()
        .map(|(i, position)| DisplayedBlock {
            position,
            state: if i == 0 { &wet_fence } else { &states[0] },
            pixels: 2,
            resolved: true,
        })
        .collect();
    owners.sort_unstable_by_key(display_order);
    assert!(!visible(&target, &owners, 1, &mut budget()).unwrap());
    for owner in &mut owners {
        owner.state = if owner.position == [223, 81, -14] {
            &states[1]
        } else {
            &invalid_timber
        };
    }
    assert!(!visible(&target, &owners, 1, &mut budget()).unwrap());
}
