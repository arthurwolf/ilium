//! Original state-aware compatibility geometry, never source-pack model JSON.
//! Texture names are explicit material bindings; unrelated IDs are rejected.
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

pub struct StateGeometry {
    pub models: BTreeMap<String, Value>,
    pub blockstate: Value,
}

const WOODS: [&str; 11] = [
    "oak", "spruce", "birch", "jungle", "acacia", "dark_oak", "mangrove", "cherry", "bamboo",
    "pale_oak", "poplar",
];
const FACING: [(&str, i32); 4] = [("north", 0), ("east", 90), ("south", 180), ("west", 270)];

fn material(prefix: &str) -> Option<String> {
    if WOODS.contains(&prefix) {
        return Some(format!("{prefix}_planks"));
    }
    Some(
        match prefix {
            "stone" => "stone",
            "cobblestone" => "cobblestone",
            "mossy_cobblestone" => "mossy_cobblestone",
            "stone_brick" => "stone_bricks",
            "mossy_stone_brick" => "mossy_stone_bricks",
            "brick" => "bricks",
            "sandstone" => "sandstone",
            "smooth_sandstone" => "sandstone_top",
            "cut_sandstone" => "cut_sandstone",
            "red_sandstone" => "red_sandstone",
            "smooth_red_sandstone" => "red_sandstone_top",
            "cut_red_sandstone" => "cut_red_sandstone",
            "granite" => "granite",
            "diorite" => "diorite",
            "andesite" => "andesite",
            "polished_granite" => "polished_granite",
            "polished_diorite" => "polished_diorite",
            "polished_andesite" => "polished_andesite",
            "cobbled_deepslate" => "cobbled_deepslate",
            "polished_deepslate" => "polished_deepslate",
            "deepslate_brick" => "deepslate_bricks",
            "deepslate_tile" => "deepslate_tiles",
            "tuff" => "tuff",
            "polished_tuff" => "polished_tuff",
            "tuff_brick" => "tuff_bricks",
            "bamboo_mosaic" => "bamboo_mosaic",
            _ => return None,
        }
        .into(),
    )
}

fn cuboid(from: [i32; 3], to: [i32; 3], texture: &str) -> Value {
    let faces: Map<String, Value> = ["down", "up", "north", "south", "west", "east"]
        .into_iter()
        .map(|face| {
            (
                face.into(),
                json!({"texture":format!("minecraft:block/{texture}")}),
            )
        })
        .collect();
    json!({"from":from,"to":to,"faces":faces})
}

// Entity atlases are sampled in the model compiler's 0..16 UV domain. The
// rectangles below name atlas pixels in a 64x64 chest or 32x32 bell; scaling
// here also keeps the same authored regions in 2x and 4x pack images.
fn atlas_cuboid(
    from: [i32; 3],
    to: [i32; 3],
    texture: &str,
    atlas_edge: i32,
    rectangles: [[i32; 4]; 6],
) -> Value {
    let faces: Map<String, Value> = ["down", "up", "north", "south", "west", "east"]
        .into_iter()
        .zip(rectangles)
        .map(|(face, rectangle)| {
            let uv = rectangle.map(|pixel| f64::from(pixel * 16) / f64::from(atlas_edge));
            (face.into(), json!({"texture":texture,"uv":uv}))
        })
        .collect();
    json!({"from":from,"to":to,"faces":faces})
}

fn chest_elements() -> Vec<Value> {
    let texture = "minecraft:entity/chest/normal";
    vec![
        atlas_cuboid(
            [1, 0, 1],
            [15, 10, 15],
            texture,
            64,
            [
                [28, 19, 42, 33],
                [14, 19, 28, 33],
                [14, 33, 28, 43],
                [42, 33, 56, 43],
                [0, 33, 14, 43],
                [28, 33, 42, 43],
            ],
        ),
        atlas_cuboid(
            [1, 10, 1],
            [15, 15, 15],
            texture,
            64,
            [
                [28, 0, 42, 14],
                [14, 0, 28, 14],
                [14, 14, 28, 19],
                [42, 14, 56, 19],
                [0, 14, 14, 19],
                [28, 14, 42, 19],
            ],
        ),
        // The separate lock is original geometry. Its narrow visible face
        // samples the opaque latch island; exact entity cuboid parity awaits
        // a final pixel review of the selected pack and renderer orientation.
        atlas_cuboid([7, 8, 0], [9, 12, 1], texture, 64, [[0, 1, 2, 5]; 6]),
    ]
}

fn bell_elements(block_faces: bool) -> Vec<Value> {
    if block_faces {
        // Plasticator supplies these three cropped 8x8 face images but no
        // bell entity atlas. Preserve their actual resource identities.
        let mut body = cuboid([5, 6, 5], [11, 13, 11], "bell_side");
        let mut lip = cuboid([4, 4, 4], [12, 6, 12], "bell_side");
        let mut stem = cuboid([7, 13, 7], [9, 16, 9], "bell_side");
        for element in [&mut body, &mut lip, &mut stem] {
            for (face, texture) in [
                ("up", "bell_top"),
                ("down", "bell_bottom"),
                ("north", "bell_side"),
                ("south", "bell_side"),
                ("west", "bell_side"),
                ("east", "bell_side"),
            ] {
                element["faces"][face] = json!({
                    "texture":format!("minecraft:block/{texture}"), "uv":[0,0,8,8]
                });
            }
        }
        return vec![body, lip, stem];
    }
    let texture = "minecraft:entity/bell/bell_body";
    vec![
        atlas_cuboid(
            [5, 6, 5],
            [11, 13, 11],
            texture,
            32,
            [
                [12, 0, 18, 6],
                [6, 0, 12, 6],
                [6, 6, 12, 13],
                [18, 6, 24, 13],
                [0, 6, 6, 13],
                [12, 6, 18, 13],
            ],
        ),
        atlas_cuboid(
            [4, 4, 4],
            [12, 6, 12],
            texture,
            32,
            [
                [16, 13, 24, 21],
                [8, 13, 16, 21],
                [8, 21, 16, 23],
                [24, 21, 32, 23],
                [0, 21, 8, 23],
                [16, 21, 24, 23],
            ],
        ),
        atlas_cuboid([7, 13, 7], [9, 16, 9], texture, 32, [[6, 0, 8, 6]; 6]),
    ]
}

fn hollow_vessel(side: &str, top: &str, bottom: &str, inner: &str) -> Vec<Value> {
    let mut floor = cuboid([0, 0, 0], [16, 4, 16], side);
    floor["faces"]["down"]["texture"] = json!(format!("minecraft:block/{bottom}"));
    floor["faces"]["up"]["texture"] = json!(format!("minecraft:block/{inner}"));
    let mut elements = vec![floor];
    for (from, to, inward) in [
        ([0, 4, 0], [16, 16, 2], "south"),
        ([0, 4, 14], [16, 16, 16], "north"),
        ([0, 4, 2], [2, 16, 14], "east"),
        ([14, 4, 2], [16, 16, 14], "west"),
    ] {
        let mut wall = cuboid(from, to, side);
        wall["faces"]["up"]["texture"] = json!(format!("minecraft:block/{top}"));
        wall["faces"][inward]["texture"] = json!(format!("minecraft:block/{inner}"));
        elements.push(wall);
    }
    elements
}

pub fn is_remaining_generated_material(id: &str) -> bool {
    matches!(
        id,
        "minecraft:campfire"
            | "minecraft:chest"
            | "minecraft:cauldron"
            | "minecraft:bell"
            | "minecraft:composter"
            | "minecraft:oak_fence_gate"
            | "minecraft:jungle_fence_gate"
            | "minecraft:acacia_fence_gate"
            | "minecraft:spruce_fence_gate"
    )
}

fn model(result: &mut StateGeometry, block: &str, suffix: &str, elements: Vec<Value>) -> String {
    let id = format!("minecraft:block/ilium_{block}_{suffix}");
    result
        .models
        .insert(id.clone(), json!({"elements":elements}));
    id
}

fn bed_cuboid(from: [i32; 3], to: [i32; 3], color: &str, leg: bool, foot: bool) -> Value {
    let mut element = cuboid(from, to, "unused");
    // Original authored upright bed adapter for the conventional 64-unit
    // entity atlas. Rectangles scale with the actual decoded pack resolution.
    // This is explicit compatibility, not proof of native entity-model parity.
    let rectangles = if leg {
        [
            [53, 0, 56, 3],
            [56, 0, 59, 3],
            [53, 3, 56, 6],
            [59, 3, 62, 6],
            [50, 3, 53, 6],
            [56, 3, 59, 6],
        ]
    } else {
        [
            [28, 6, 44, 22],
            [6, 6, 22, 22],
            [6, 0, 22, 6],
            [22, 0, 38, 6],
            [0, 6, 6, 22],
            [22, 6, 28, 22],
        ]
    };
    for (face, rect) in ["down", "up", "north", "south", "west", "east"]
        .into_iter()
        .zip(rectangles)
    {
        let uv = std::array::from_fn::<_, 4, _>(|index| {
            let offset = if foot && !leg && index % 2 == 1 {
                22
            } else {
                0
            };
            f64::from(rect[index] + offset) / 4.0
        });
        element["faces"][face] = json!({"texture":format!("minecraft:entity/bed/{color}"),"uv":uv});
    }
    element
}

fn cross(texture: &str, tinted: bool) -> Vec<Value> {
    [-45,45].into_iter().map(|angle| {
        let mut face = json!({"texture":format!("minecraft:block/{texture}"),"uv":[0,0,16,16]});
        if tinted {face["tintindex"] = json!(0);}
        json!({"from":[0,0,8],"to":[16,16,8],"shade":false,"rotation":{"origin":[8,8,8],"axis":"y","angle":angle,"rescale":false},"faces":{"north":face,"south":face}})
    }).collect()
}

// The selected pack's campfire_log atlas has bark at [0,0,16,4] and a
// 4x4 end-grain island at [0,4,4,8]. Its remaining cutout/dark pixels are
// not a rectangular solid-log face. Keep the original pack image unchanged.
fn campfire_log_element(from: [i32; 3], to: [i32; 3], texture: &str, along_x: bool) -> Value {
    let mut element = cuboid(from, to, texture);
    for face in ["down", "up", "north", "south", "west", "east"] {
        let is_end = if along_x {
            matches!(face, "west" | "east")
        } else {
            matches!(face, "north" | "south")
        };
        element["faces"][face]["uv"] = if is_end {
            json!([0, 4, 4, 8])
        } else {
            json!([0, 0, 16, 4])
        };
    }
    // Up/down face U runs along X. A Z-axis log must rotate its bark strip
    // so the source's 16-pixel direction follows the log, not its 4-pixel width.
    if !along_x {
        for face in ["down", "up"] {
            element["faces"][face]["rotation"] = json!(90);
        }
    }
    element
}

fn campfire_elements(lit: bool) -> Vec<Value> {
    let texture = if lit {
        "campfire_log_lit"
    } else {
        "campfire_log"
    };
    // Two bottom E-W logs support two N-S logs. All are four texels thick;
    // the 8/16-high silhouette stays a low pile in every facing.
    let mut elements = vec![
        campfire_log_element([0, 0, 1], [16, 4, 5], texture, true),
        campfire_log_element([0, 0, 11], [16, 4, 15], texture, true),
        campfire_log_element([1, 4, 0], [5, 8, 16], texture, false),
        campfire_log_element([11, 4, 0], [15, 8, 16], texture, false),
    ];
    if lit {
        // Retain the existing two crossed, double-sided flame sheets and
        // their small depth offsets; only solid-log geometry is corrected.
        for fire in cross("campfire_fire", false) {
            for offset in [-0.125, 0.125] {
                let mut face = fire.clone();
                face["from"][2] = json!(8.0 + offset);
                face["to"][2] = json!(8.0 + offset);
                elements.push(face);
            }
        }
    }
    elements
}

fn ground_patch(texture: &str, rectangle: [i32; 4], height: f64, tinted: bool) -> Value {
    let [x0, z0, x1, z1] = rectangle;
    let mut face = json!({"texture":format!("minecraft:block/{texture}"),"uv":[x0,z0,x1,z1]});
    if tinted {
        face["tintindex"] = json!(0);
    }
    json!({"from":[x0,height,z0],"to":[x1,height,z1],"shade":false,"faces":{"up":face,"down":face}})
}

fn attached_plane(texture: &str, height: i32, tinted: bool) -> Value {
    let mut face = json!({"texture":format!("minecraft:block/{texture}"),"uv":[0,16-height,16,16]});
    if tinted {
        face["tintindex"] = json!(0);
    }
    json!({"from":[0,0,0.8],"to":[16,height,0.8],"shade":false,"faces":{"north":face,"south":face}})
}

// Canonical fallback names preserve selected model-only overrides as well as blockstates.
fn workstation_model(
    // Register original geometry inside the existing definition provider.
    result: &mut StateGeometry, // Retain the caller-owned, bounded definition collection.
    name: &str,                 // Use a canonical model name, never a texture alias.
    elements: Vec<Value>,       // Keep authored elements separate from selected source JSON.
) -> String {
    // Return the same ID that selected-first model lookup will resolve.
    let id = format!("minecraft:block/{name}"); // Selected models can override this exact key.
    result
        .models
        .insert(id.clone(), json!({"elements":elements})); // Missing models use this original fallback.
    id // Keep model and texture identities distinct.
} // Preserve the existing public factory API.

fn workstation_box(
    // Bind six explicit face materials without changing the shared cuboid helper.
    from: [i32; 3], // Use Java model coordinates within the existing compiler's limits.
    to: [i32; 3],   // Keep each authored box finite and ordered.
    textures: [&str; 6], // Order faces as down, up, north, south, west, east.
) -> Value {
    // Return ordinary model JSON for the existing compiler.
    let mut element = cuboid(from, to, textures[0]); // Reuse the established element construction.
    for (face, texture) in ["down", "up", "north", "south", "west", "east"] // Name every face explicitly.
        .into_iter() // Visit the fixed six-face set only.
        .zip(textures)
    // Associate each face with its real material resource.
    {
        // Restrict assignments to this new workstation element.
        element["faces"][face] = json!({ // Make the full-face UV domain explicit by default.
            "texture":format!("minecraft:block/{texture}"), "uv":[0,0,16,16] // Cropped parts override these UVs below.
        }); // Leave source alpha, layers and animation to the unchanged importer.
    } // Complete the fixed face binding.
    element // Do not install a semantic texture alias.
} // Existing workstation and prop helpers remain unchanged.

fn workstation_brewing_rod(from: [i32; 3], to: [i32; 3]) -> Value {
    // Reuse evidenced rod material on original hardware.
    let lengths = std::array::from_fn::<_, 3, _>(|axis| to[axis] - from[axis]); // The fixed hardware always has one longest axis.
    let axis = if lengths[0] > lengths[1] && lengths[0] > lengths[2] {
        // Detect the east arm's longitudinal axis.
        0 // Its cap artwork belongs on west and east.
    } else if lengths[2] > lengths[1] {
        // Detect the north and south arms.
        2 // Their cap artwork belongs on north and south.
    } else {
        // The stem and hooks remain upright.
        1 // Their cap artwork belongs on down and up.
    }; // This local choice does not add a model or importer API.
    let (negative, positive) = [("west", "east"), ("down", "up"), ("north", "south")][axis]; // Keep caps on the actual rod ends.
    let mut element = cuboid(from, to, "brewing_stand"); // The supplied model establishes the rod atlas material.
    for face in ["down", "up", "north", "south", "west", "east"] {
        // Map all six surfaces for this hardware orientation.
        let end = face == negative || face == positive; // Separate axial ends from longitudinal faces.
        let uv = if face == negative {
            // Use the witnessed lower cap on the negative end.
            [6, 14, 8, 16] // Keep the cap inside its two-by-two source island.
        } else if face == positive {
            // Use the witnessed upper cap on the positive end.
            [6, 2, 8, 4] // Preserve the other cap's independent source region.
        } else {
            // All remaining faces run along the rod.
            [7, 2, 9, 16] // Use the evidenced long metal strip without guessing bottle islands.
        }; // The same source regions serve the original empty hardware.
        element["faces"][face]["uv"] = json!(uv); // Retain the compiler's 16-unit UV convention.
        if !end && (axis == 0 || (axis == 2 && matches!(face, "west" | "east"))) {
            // Align the strip's long V direction with horizontal geometry.
            element["faces"][face]["rotation"] = json!(90); // Top and bottom Z-axis faces already run longitudinally in V.
        } // Cap UVs stay unrotated.
    } // Source alpha and image metadata remain unchanged.
    element // Return a solid piece of empty-stand hardware.
} // Selected remodeled rods retain their own canonical source model.

fn workstation_brewing_elements() -> Vec<Value> {
    // Build an original stem and three low stone feet.
    let mut elements = vec![workstation_brewing_rod([7, 0, 7], [9, 14, 9])]; // Give the narrow stem a positive-area joint with all three feet.
    for (from, to, top_uv, side_uv) in [
        // Use only the base regions witnessed by the supplied model.
        ([9, 0, 5], [14, 2, 11], [8, 4, 14, 10], [9, 14, 15, 16]), // East foot touches the stem.
        ([3, 0, 2], [8, 2, 7], [2, 2, 8, 8], [2, 14, 8, 16]), // Northwest foot joins the stem's north side.
        ([3, 0, 9], [8, 2, 14], [2, 8, 8, 14], [2, 14, 8, 16]), // Southwest foot stays separate and joins the south side.
    ] {
        // Construct three bounded base pieces.
        elements.push(atlas_cuboid(
            // Preserve cropped top and thin side materials.
            from,                                 // Use the authored foot origin.
            to,                                   // Keep each foot two units high.
            "minecraft:block/brewing_stand_base", // Missing art follows the existing explicit fallback policy.
            16, // Use model-space UV units at every pack resolution.
            [top_uv, top_uv, side_uv, side_uv, side_uv, side_uv], // Caps and sides sample their own evidenced regions.
        )); // No texture pixels or metadata are synthesized.
    } // Complete the finite tripod.
    elements // Empty-slot hardware is a separate canonical multipart companion.
} // Whimscape's selected base instead retains its remodeled atlas and rod.

fn workstation_brewing_empty(slot: usize) -> Vec<Value> {
    // Empty slots have hardware, never fictitious bottles.
    let (arm_from, arm_to, hook_from, hook_to) = match slot {
        // Match only the three known slot identities.
        0 => ([9, 10, 7], [12, 12, 9], [11, 6, 7], [12, 10, 9]), // East support.
        1 => ([7, 10, 4], [9, 12, 7], [7, 6, 4], [9, 10, 5]),    // North support.
        2 => ([7, 10, 9], [9, 12, 12], [7, 6, 11], [9, 10, 12]), // South support.
        _ => return Vec::new(), // Reject an out-of-range internal slot without indexing or allocation.
    }; // These are authored bare supports using the evidenced rod material.
    vec![
        // Keep each empty companion small and independently selectable.
        workstation_brewing_rod(arm_from, arm_to), // The arm attaches to the central stem.
        workstation_brewing_rod(hook_from, hook_to), // The downward hook makes the empty slot recognizable.
    ] // Selected empty companions can replace this hardware through ordinary lookup.
} // Occupied companions are never replaced by this empty geometry.

fn workstation_lectern_elements() -> Vec<Value> {
    // Author the bookless stand supported by the actual village states.
    let mut base = workstation_box(
        // Give the foot its own rim, top and underside materials.
        [1, 0, 1],   // Leave a small original inset around the foot.
        [15, 2, 15], // Keep the base low.
        [
            "oak_planks",
            "lectern_base",
            "lectern_base",
            "lectern_base",
            "lectern_base",
            "lectern_base",
        ], // Source models establish the wood underside.
    ); // No book image is requested by a bookless lectern.
    for face in ["north", "south", "west", "east"] {
        // Crop the thin foot rim rather than stretching the whole image.
        base["faces"][face]["uv"] = json!([0, 11, 16, 13]); // The supplied Whimscape model identifies this base-rim region.
    } // Leave the top and underside as full faces.
    let mut stem = workstation_box(
        // A narrow post leaves the reading surface visibly overhanging.
        [5, 2, 6],    // Join the base without using a full cube.
        [11, 12, 10], // The top intersects the sloped desk from beneath.
        [
            "oak_planks",
            "oak_planks",
            "lectern_front",
            "lectern_front",
            "lectern_sides",
            "lectern_sides",
        ], // Keep front and lateral artwork distinct.
    ); // Crop within the supplied front and side atlas regions.
    stem["faces"]["north"]["uv"] = json!([1, 1, 7, 11]); // Use the front half of lectern_front.
    stem["faces"]["south"]["uv"] = json!([9, 5, 15, 15]); // Use the distinct rear half.
    for face in ["west", "east"] {
        // Turn the side grain with the upright post.
        stem["faces"][face]["uv"] = json!([2, 8, 12, 12]); // Stay inside the witnessed side-post region.
        stem["faces"][face]["rotation"] = json!(90); // Preserve the source side-region orientation.
    } // End the two lateral assignments.
    let mut desk = workstation_box(
        // The reading idea is an actual tilted slab, not a tall cube.
        [1, 11, 2], // Use original dimensions that remain inside the block after rotation.
        [15, 13, 14], // Keep the desk two units thick.
        [
            "oak_planks",
            "lectern_top",
            "lectern_front",
            "lectern_sides",
            "lectern_sides",
            "lectern_sides",
        ], // Preserve the witnessed front edge and lateral wood materials.
    ); // Explicit UVs stay attached to the tilted wood.
    desk["rotation"] = json!({"origin":[8,12,8],"axis":"x","angle":-22.5,"rescale":false}); // Use the supported slope witnessed in both supplied lecterns.
    desk["faces"]["up"]["uv"] = json!([1, 0, 15, 12]); // Crop the reading surface within its documented region.
    desk["faces"]["up"]["rotation"] = json!(180); // Keep the front-facing reading orientation.
    desk["faces"]["down"]["uv"] = json!([1, 2, 15, 14]); // Keep the wood underside proportional to the desk.
    for face in ["north", "south"] {
        // The two wide edges sample only edge material.
        desk["faces"][face]["uv"] = json!([1, 0, 15, 2]); // Crop the independently witnessed front and rear edge bands.
    } // Complete the wide edges.
    for face in ["west", "east"] {
        // The two short edges retain their own extent.
        desk["faces"][face]["uv"] = json!([0, 4, 12, 6]); // Avoid stretching an entire atlas over an edge.
    } // No uvlock is enabled on this non-axis-aligned element.
    vec![base, stem, desk] // Occupied book rendering requires a separately evidenced entity-art contract.
} // Do not substitute lectern wood for missing book pages.

fn workstation_stonecutter_elements() -> Vec<Value> {
    // Build a low body with a separate two-sided saw.
    let mut body = workstation_box(
        // Bind stone sides, worktop and underside independently.
        [0, 0, 0],   // Sit on the floor plane.
        [16, 8, 16], // Keep the body half a block high.
        [
            "stonecutter_bottom",
            "stonecutter_top",
            "stonecutter_side",
            "stonecutter_side",
            "stonecutter_side",
            "stonecutter_side",
        ], // Native overrides can retain additional side materials.
    ); // Selected source models retain their own dimensions and tint metadata.
    for face in ["north", "south", "west", "east"] {
        // Preserve the lower-half side-image crop.
        body["faces"][face]["uv"] = json!([0, 8, 16, 16]); // The supplied model establishes this body region.
    } // Keep full UVs for the top and bottom.
    let saw = json!({ // Use a true vertical plane with independently directed faces.
        "from":[1,8,8], "to":[15,16,8], // Inset the original blade one unit at each end.
        "faces":{ // Source alpha and animation remain importer-owned.
            "north":{"texture":"minecraft:block/stonecutter_saw","uv":[1,8,15,16]}, // Show the evidenced lower-half blade region.
            "south":{"texture":"minecraft:block/stonecutter_saw","uv":[15,8,1,16]} // Mirror the reverse view of the same blade.
        } // No solid all-in-one stonecutter texture is requested.
    }); // Zero thickness is supported by the existing model compiler.
    vec![body, saw] // Retain a recognizable body/blade silhouette.
} // Do not force alpha or create a saw animation schedule.

fn workstation_grindstone_elements() -> Vec<Value> {
    // Author a stepped wheel carried by two wooden supports.
    let mut elements = vec![
        // Keep the foot, pivot and abrasive surfaces separate.
        cuboid([2, 0, 6], [4, 9, 10], "stripped_spruce_log"), // The Jicklus model grounds this wooden support material.
        cuboid([12, 0, 6], [14, 9, 10], "stripped_spruce_log"), // Selected models keep their own support material instead.
    ]; // Auxiliary wood availability remains an explicit import requirement.
    for (from, to) in [([2, 8, 6], [5, 12, 10]), ([11, 8, 6], [14, 12, 10])] {
        // Join both supports to the wheel axle.
        elements.push(atlas_cuboid(
            // Pivot regions come from all three supplied grindstone models.
            from,                               // Use an original housing position.
            to,                                 // Keep the housing compact.
            "minecraft:block/grindstone_pivot", // Never smear wheel art down the wooden legs.
            16, // Preserve the source UV domain at every resolution.
            [
                [8, 0, 10, 6],
                [8, 0, 10, 6],
                [6, 0, 8, 6],
                [6, 0, 8, 6],
                [0, 0, 6, 6],
                [0, 0, 6, 6],
            ], // Caps, thin sides and axle ends have separate regions.
        )); // Complete one housing.
    } // The axle remains along X before blockstate orientation.
    for (y0, y1, z0, z1) in [(4, 6, 5, 11), (6, 14, 2, 14), (14, 16, 5, 11)] {
        // Three bands make an original stepped round silhouette.
        let mut wheel = workstation_box(
            // Use an extruded wheel rather than a generic block cube.
            [5, y0, z0],  // Keep both disk faces between the support housings.
            [11, y1, z1], // Preserve a bounded twelve-unit wheel height.
            [
                "grindstone_round",
                "grindstone_round",
                "grindstone_round",
                "grindstone_round",
                "grindstone_side",
                "grindstone_side",
            ], // East/west are disks; the other faces are the abrasive rim.
        ); // Crop all three disk bands from one continuous source disk region.
        for face in ["west", "east"] {
            // Avoid repeating a complete disk texture on every band.
            wheel["faces"][face]["uv"] = json!([z0 - 2, 16 - y1, z1 - 2, 16 - y0]);
            // Stay within the witnessed twelve-by-twelve disk region.
        } // The cropped bands preserve continuous disk artwork.
        for face in ["down", "up", "north", "south"] {
            // Keep the rim on its evidenced abrasive region.
            wheel["faces"][face]["uv"] = json!([0, 0, 8, 12]); // Do not sample the unused remainder of the atlas.
        } // Source-native rims retain their selected UVs when present.
        let faces = wheel["faces"]
            .as_object_mut()
            .expect("workstation_box creates faces"); // The local constructor always creates this object.
        if y0 == 4 {
            // The lowest band touches the middle band on its upper face.
            faces.remove("up"); // Avoid a duplicate coplanar face inside the wheel.
        } // Preserve the visible lower ledges of the middle band.
        if y1 == 16 {
            // The uppermost band touches the middle band on its lower face.
            faces.remove("down"); // Avoid the second duplicate interior face.
        } // Preserve the middle band's exposed upper ledges.
        elements.push(wheel); // Retain each original band as a bounded compiler element.
    } // Complete seven total elements: two legs, two housings and three wheel bands.
    elements // Blockstate rotations move the complete support/wheel relationship together.
} // Selected grindstone models retain their own topology and texture paths.

fn workstation_definitions(block: &str) -> Option<StateGeometry> {
    // Extend the existing registry for only the nine missing jobs.
    if !matches!(
        // Reject unrelated IDs before constructing any workstation metadata.
        block, // The caller has already checked the minecraft namespace.
        "blast_furnace" | "smoker" | "cartography_table" | "brewing_stand" // Keep the allowlist finite.
            | "lectern" | "stonecutter" | "loom" | "smithing_table" | "grindstone" // Leave all four prior factories on their exact existing branches.
    ) {
        // Preserve every other definition unchanged.
        return None; // Do not infer compatibility from filename resemblance.
    } // Only supported workstation identities reach the new factories.
    let mut result = StateGeometry {
        // Use the same model/state container as existing factories.
        models: BTreeMap::new(), // Deterministic canonical model ordering.
        blockstate: Value::Null, // Set exactly one variants or multipart definition below.
    }; // Allocation remains finite and registration keeps the existing shared budget.
    let mut variants = Map::new(); // Preserve deterministic selector construction.
    match block {
        // Dispatch only to the existing architecture's original geometry forms.
        "blast_furnace" | "smoker" => {
            // Both appliances have a meaningful front and lit state.
            for lit in [false, true] {
                // The two states share the same solid dimensions.
                let name = if lit {
                    format!("{block}_on")
                } else {
                    block.to_owned()
                }; // Preserve canonical selected model names.
                let side = format!("{block}_side"); // Bind the actual side image.
                let top = format!("{block}_top"); // Bind the actual upper face image.
                let bottom = if block == "smoker" {
                    "smoker_bottom"
                } else {
                    "blast_furnace_top"
                }; // Only the smoker has a distinct bottom image.
                let front = format!("{block}_front{}", if lit { "_on" } else { "" }); // Lit changes only the front artwork.
                let element = workstation_box(
                    [0, 0, 0],
                    [16, 16, 16],
                    [bottom, &top, &front, &side, &side, &side],
                ); // Use complete solid appliance geometry.
                let id = workstation_model(&mut result, &name, vec![element]); // Selected native models remain first.
                for (facing, angle) in FACING {
                    // Cover every actual village rotation.
                    variants.insert(
                        format!("facing={facing},lit={lit}"),
                        json!({"model":id,"y":angle}),
                    ); // Keep UVs attached to the oriented front.
                } // Complete the four directions for this lit state.
            } // No playback metadata is fabricated for lit images.
        } // Finish the two appliance factories.
        "cartography_table" | "loom" | "smithing_table" => {
            // Tables bind distinct whole-face materials.
            let textures = match block {
                // Use only exact evidenced face resources and an explicit wooden underside.
                "cartography_table" => [
                    "dark_oak_planks",
                    "cartography_table_top",
                    "cartography_table_side1",
                    "cartography_table_side3",
                    "cartography_table_side2",
                    "cartography_table_side3",
                ], // Cardinal placement is original, not a claim of native map orientation.
                "loom" => [
                    "loom_bottom",
                    "loom_top",
                    "loom_front",
                    "loom_side",
                    "loom_side",
                    "loom_side",
                ], // Keep the loom's patterned front distinct.
                _ => [
                    "smithing_table_bottom",
                    "smithing_table_top",
                    "smithing_table_front",
                    "smithing_table_front",
                    "smithing_table_side",
                    "smithing_table_side",
                ], // Both tool faces retain the front material.
            }; // Do not introduce item-only or numbered alternate texture aliases.
            let element = workstation_box([0, 0, 0], [16, 16, 16], textures); // A full workbench is appropriate for these three shapes.
            let id = workstation_model(&mut result, block, vec![element]); // Retain canonical model-only overrides.
            if block == "loom" {
                // The current loom state includes a horizontal facing.
                for (facing, angle) in FACING {
                    // Orient the patterned front in every village rotation.
                    variants.insert(format!("facing={facing}"), json!({"model":id,"y":angle}));
                    // No unused facing property is invented for the other tables.
                } // Complete the loom directions.
            } else {
                // Cartography and smithing states have no required properties.
                variants.insert(String::new(), json!({"model":id})); // Preserve their actual propertyless generated states.
            } // Finish the appropriate selector form.
        } // Finish the three full workbenches.
        "brewing_stand" => {
            // Preserve independent bottle flags through ordinary multipart selection.
            let id = workstation_model(&mut result, block, workstation_brewing_elements()); // The selected canonical base can replace this fallback.
            let mut parts = vec![json!({"apply":{"model":id}})]; // Always include the central stand.
            for slot in 0..3 {
                // Register only the three existing slot identities.
                let empty_name = format!("brewing_stand_empty{slot}"); // Preserve exact canonical companion spelling.
                let empty =
                    workstation_model(&mut result, &empty_name, workstation_brewing_empty(slot)); // Missing empty companions use original bare hardware.
                let property = format!("has_bottle_{slot}"); // Use the real blockstate property.
                parts.push(json!({"when":{(property.clone()):"false"},"apply":{"model":empty}})); // Empty slots retain selected-native precedence.
                let occupied = format!("minecraft:block/brewing_stand_bottle{slot}"); // Whimscape supplies the evidenced occupied companion.
                parts.push(json!({"when":{property:"true"},"apply":{"model":occupied}}));
                // Missing occupied artwork must remain an explicit missing-model result.
            } // Never install an empty or guessed fallback for a true bottle flag.
            result.blockstate = json!({"multipart":parts}); // Use the existing multipart selector implementation.
            return Some(result); // Do not overwrite multipart with the empty variants map.
        } // Complete all actual generated empty-stand states and native occupied companions.
        "lectern" => {
            // The generated stand is bookless; a book needs its own evidenced entity-art layout.
            let id = workstation_model(&mut result, block, workstation_lectern_elements()); // Preserve source-model-only lectern overrides.
            for (facing, angle) in FACING {
                // Cover every direction written by village assembly.
                variants.insert(
                    format!("facing={facing},has_book=false"),
                    json!({"model":id,"y":angle}),
                ); // Powered stays geometrically inert; occupied books are not falsely accepted.
            } // Omit uvlock because the reading surface is sloped.
        } // Keep unsupported occupied-book states explicit.
        "stonecutter" => {
            // A low worktop and blade replace the erroneous generic cube.
            let id = workstation_model(&mut result, block, workstation_stonecutter_elements()); // Native extra side art and saw tint remain authoritative.
            for (facing, angle) in FACING {
                // Rotate the blade with the complete workstation.
                variants.insert(format!("facing={facing}"), json!({"model":id,"y":angle}));
                // Preserve the actual single-property state form.
            } // Complete all four orientations.
        } // Leave source animation schedules unchanged.
        "grindstone" => {
            // One original wheel/support model serves all attachment planes.
            let id = workstation_model(&mut result, block, workstation_grindstone_elements()); // Native selected wheel models retain precedence.
            for (face, x) in [("floor", 0), ("wall", 90), ("ceiling", 180)] {
                // Move the feet to the correct supporting plane.
                for (facing, y) in FACING {
                    // Preserve coherent wheel and support orientation.
                    variants.insert(
                        format!("face={face},facing={facing}"),
                        json!({"model":id,"x":x,"y":y}),
                    ); // The property is face, not bell's attachment.
                } // Finish all four directions for this mounting plane.
            } // Keep all twelve meaningful mounting variants.
        } // Finish the final missing profession factory.
        _ => return None, // Retain an exhaustive guard if the allowlist is later edited.
    } // No existing workstation or prior prop branch was modified.
    result.blockstate = json!({"variants":variants}); // Publish the completed selector JSON to the existing registrar.
    Some(result) // Resource validation, cancellation and charging remain with the established consumers.
} // End the bounded private factory extension.

pub fn definitions(id: &str) -> Option<StateGeometry> {
    definitions_for_profile(id, false)
}

/// Plasticator's bell art is split into three block-face images, whereas the
/// other admitted full packs with bell art supply an entity atlas.
pub fn definitions_for_profile(id: &str, plasticator_bell_faces: bool) -> Option<StateGeometry> {
    let block = id.strip_prefix("minecraft:")?;
    if let Some(workstation) = workstation_definitions(block) {
        // Resolve only the nine new canonical workstation factories.
        return Some(workstation); // Preserve all other definition branches byte-for-byte.
    } // Keep the prior material and Saved selection policies in their existing consumers.
    let mut result = StateGeometry {
        models: BTreeMap::new(),
        blockstate: Value::Null,
    };
    let mut variants = Map::new();
    if block == "campfire" {
        for lit in [false, true] {
            let id = model(
                &mut result,
                block,
                &format!("lit_{lit}"),
                campfire_elements(lit),
            );
            for (facing, angle) in FACING {
                variants.insert(
                    format!("facing={facing},lit={lit}"),
                    json!({"model":id,"y":angle}),
                );
            }
        }
    } else if block == "chest" {
        let id = model(&mut result, block, "single", chest_elements());
        for (facing, angle) in FACING {
            variants.insert(
                format!("facing={facing},type=single"),
                json!({"model":id,"y":angle}),
            );
        }
    } else if block == "cauldron" {
        let id = model(
            &mut result,
            block,
            "empty",
            hollow_vessel(
                "cauldron_side",
                "cauldron_top",
                "cauldron_bottom",
                "cauldron_inner",
            ),
        );
        variants.insert(String::new(), json!({"model":id}));
    } else if block == "bell" {
        let id = model(
            &mut result,
            block,
            "ceiling",
            bell_elements(plasticator_bell_faces),
        );
        for (facing, angle) in FACING {
            variants.insert(
                format!("attachment=ceiling,facing={facing}"),
                json!({"model":id,"y":angle}),
            );
        }
    } else if block == "composter" {
        for level in 0..=8 {
            let mut elements = hollow_vessel(
                "composter_side",
                "composter_top",
                "composter_bottom",
                "composter_side",
            );
            if level > 0 {
                let content = if level == 8 {
                    "composter_ready"
                } else {
                    "composter_compost"
                };
                let height = 4 + level;
                let mut fill = cuboid([2, 4, 2], [14, height, 14], "composter_side");
                fill["faces"]["up"]["texture"] = json!(format!("minecraft:block/{content}"));
                elements.push(fill);
            }
            let id = model(&mut result, block, &format!("level_{level}"), elements);
            variants.insert(format!("level={level}"), json!({"model":id}));
        }
    } else if matches!(block, "hay_block" | "bone_block") {
        let mut element = cuboid([0, 0, 0], [16, 16, 16], &format!("{block}_side"));
        for face in ["up", "down"] {
            element["faces"][face]["texture"] = json!(format!("minecraft:block/{block}_top"));
        }
        let id = model(&mut result, block, "column", vec![element]);
        for (axis, x, y) in [("y", 0, 0), ("x", 90, 90), ("z", 90, 0)] {
            variants.insert(format!("axis={axis}"), json!({"model":id,"x":x,"y":y}));
        }
    } else if block == "barrel" {
        for open in [false, true] {
            let mut element = cuboid([0, 0, 0], [16, 16, 16], "barrel_side");
            element["faces"]["down"]["texture"] = json!("minecraft:block/barrel_bottom");
            element["faces"]["up"]["texture"] = json!(if open {
                "minecraft:block/barrel_top_open"
            } else {
                "minecraft:block/barrel_top"
            });
            let id = model(&mut result, block, &format!("open_{open}"), vec![element]);
            for (facing, x, y) in [
                ("up", 0, 0),
                ("down", 180, 0),
                ("north", 90, 0),
                ("east", 90, 90),
                ("south", 90, 180),
                ("west", 90, 270),
            ] {
                variants.insert(
                    format!("facing={facing},open={open}"),
                    json!({"model":id,"x":x,"y":y}),
                );
            }
        }
    } else if block == "furnace" {
        for lit in [false, true] {
            let mut element = cuboid([0, 0, 0], [16, 16, 16], "furnace_side");
            for face in ["up", "down"] {
                element["faces"][face]["texture"] = json!("minecraft:block/furnace_top");
            }
            element["faces"]["north"]["texture"] = json!(if lit {
                "minecraft:block/furnace_front_on"
            } else {
                "minecraft:block/furnace_front"
            });
            let id = model(&mut result, block, &format!("lit_{lit}"), vec![element]);
            for (facing, angle) in FACING {
                variants.insert(
                    format!("facing={facing},lit={lit}"),
                    json!({"model":id,"y":angle}),
                );
            }
        }
    } else if matches!(block, "crafting_table" | "fletching_table") {
        let mut element = cuboid([0, 0, 0], [16, 16, 16], &format!("{block}_side"));
        element["faces"]["up"]["texture"] = json!(format!("minecraft:block/{block}_top"));
        element["faces"]["down"]["texture"] = json!(if block == "crafting_table" {
            "minecraft:block/oak_planks"
        } else {
            "minecraft:block/birch_planks"
        });
        for face in ["north", "south"] {
            element["faces"][face]["texture"] = json!(format!("minecraft:block/{block}_front"));
        }
        let id = model(&mut result, block, "workbench", vec![element]);
        variants.insert(String::new(), json!({"model":id}));
    } else if block == "target" {
        let mut element = cuboid([0, 0, 0], [16, 16, 16], "target_side");
        for face in ["up", "down"] {
            element["faces"][face]["texture"] = json!("minecraft:block/target_top");
        }
        let id = model(&mut result, block, "target", vec![element]);
        variants.insert(String::new(), json!({"model":id}));
    } else if block == "dirt_path" {
        let mut element = cuboid([0, 0, 0], [16, 15, 16], "dirt_path_side");
        element["faces"]["up"]["texture"] = json!("minecraft:block/dirt_path_top");
        element["faces"]["down"]["texture"] = json!("minecraft:block/dirt");
        let id = model(&mut result, block, "lowered", vec![element]);
        variants.insert(String::new(), json!({"model":id}));
    } else if block == "smooth_sandstone" {
        let id = model(
            &mut result,
            block,
            "smooth",
            vec![cuboid([0, 0, 0], [16, 16, 16], "sandstone_top")],
        );
        variants.insert(String::new(), json!({"model":id}));
    } else if block == "wheat" {
        for age in 0..=7 {
            let id = model(
                &mut result,
                block,
                &format!("age_{age}"),
                cross(&format!("wheat_stage{age}"), false),
            );
            variants.insert(format!("age={age}"), json!({"model":id}));
        }
    } else if matches!(block, "torchflower" | "golden_dandelion" | "wither_rose") {
        let id = model(&mut result, block, "cross", cross(block, false));
        variants.insert(String::new(), json!({"model":id}));
    } else if matches!(block, "azalea" | "flowering_azalea") {
        // The original canopy and rooted stem artwork remain separate materials.
        // Crop the side atlas to its leafy head and the stem atlas to its base.
        let mut head = cuboid([0, 6, 0], [16, 16, 16], &format!("{block}_side"));
        for face in ["up", "down"] {
            head["faces"][face]["texture"] = json!(format!("minecraft:block/{block}_top"));
            head["faces"][face]["uv"] = json!([0, 0, 16, 16]);
        }
        for face in ["north", "south", "east", "west"] {
            head["faces"][face]["uv"] = json!([0, 0, 16, 10]);
        }
        let mut elements = vec![head];
        let mut stem = cross("azalea_plant", false);
        for element in &mut stem {
            element["to"][1] = json!(6);
            for face in ["north", "south"] {
                element["faces"][face]["uv"] = json!([0, 10, 16, 16]);
            }
        }
        elements.extend(stem);
        let id = model(&mut result, block, "rooted_shrub", elements);
        variants.insert(String::new(), json!({"model":id}));
    } else if block == "cactus" {
        let mut element = cuboid([1, 0, 1], [15, 16, 15], "cactus_side");
        for (face, texture) in [("up", "cactus_top"), ("down", "cactus_bottom")] {
            element["faces"][face]["texture"] = json!(format!("minecraft:block/{texture}"));
        }
        let id = model(&mut result, block, "inset", vec![element]);
        variants.insert(String::new(), json!({"model":id}));
    } else if matches!(block, "melon_stem" | "pumpkin_stem") {
        // Crop stems are plants with an age, unlike axis-oriented woody stems.
        // Original cross geometry crops the pack image from its rooted bottom.
        for age in 0..=7 {
            let height = (age + 1) * 2;
            let mut elements = cross(block, true);
            for element in &mut elements {
                element["to"][1] = json!(height);
                for face in ["north", "south"] {
                    element["faces"][face]["uv"] = json!([0, 16 - height, 16, 16]);
                }
            }
            let id = model(&mut result, block, &format!("age_{age}"), elements);
            variants.insert(format!("age={age}"), json!({"model":id}));
        }
    } else if block == "sweet_berry_bush" {
        // Each growth stage has its own pack image; there is no unstaged atlas.
        for age in 0..=3 {
            let texture = format!("sweet_berry_bush_stage{age}");
            let id = model(
                &mut result,
                block,
                &format!("age_{age}"),
                cross(&texture, false),
            );
            variants.insert(format!("age={age}"), json!({"model":id}));
        }
    } else if matches!(block, "leaf_litter" | "pink_petals" | "wildflowers") {
        let amount_property = if block == "leaf_litter" {
            "segment_amount"
        } else {
            "flower_amount"
        };
        let patches = [[0, 0, 8, 8], [8, 0, 16, 8], [8, 8, 16, 16], [0, 8, 8, 16]];
        for amount in 1..=4 {
            let elements = patches
                .iter()
                .take(amount)
                .flat_map(|&patch| {
                    let mut elements = vec![ground_patch(
                        block,
                        patch,
                        if block == "leaf_litter" { 0.125 } else { 2.0 },
                        block == "leaf_litter",
                    )];
                    if block != "leaf_litter" {
                        let [x0,z0,x1,z1] = patch;
                        let face = json!({"texture":format!("minecraft:block/{block}_stem"),"uv":[x0,z0,x1,z1],"tintindex":0});
                        elements.push(json!({"from":[(x0+x1)/2,0,z0],"to":[(x0+x1)/2,2,z1],"shade":false,"faces":{"east":face,"west":face}}));
                    }
                    elements
                })
                .collect();
            let id = model(&mut result, block, &format!("amount_{amount}"), elements);
            for (facing, angle) in FACING {
                variants.insert(
                    format!("{amount_property}={amount},facing={facing}"),
                    json!({"model":id,"y":angle}),
                );
            }
        }
    } else if block == "lily_pad" {
        let id = model(
            &mut result,
            block,
            "floating",
            vec![ground_patch(block, [0, 0, 16, 16], 0.25, true)],
        );
        variants.insert(
            String::new(),
            json!(FACING.map(|(_, angle)| json!({"model":id,"y":angle}))),
        );
    } else if block == "moss_carpet" {
        let id = model(
            &mut result,
            block,
            "thin",
            vec![cuboid([0, 0, 0], [16, 1, 16], "moss_block")],
        );
        variants.insert(String::new(), json!({"model":id}));
    } else if block == "pale_moss_carpet" {
        let bottom = model(
            &mut result,
            block,
            "thin",
            vec![cuboid([0, 0, 0], [16, 1, 16], block)],
        );
        let empty =
            json!({"bottom":"false","east":"none","north":"none","south":"none","west":"none"});
        let mut parts = vec![
            json!({"when":{"bottom":"true"},"apply":{"model":bottom}}),
            json!({"when":empty,"apply":{"model":bottom}}),
        ];
        for (state, height, texture) in [
            ("low", 8, "pale_moss_carpet_side_small"),
            ("tall", 16, "pale_moss_carpet_side_tall"),
        ] {
            let id = model(
                &mut result,
                block,
                state,
                vec![attached_plane(texture, height, false)],
            );
            for (facing, angle) in FACING {
                parts.push(json!({"when":{facing:state},"apply":{"model":id,"y":angle}}));
                if state == "tall" {
                    parts.push(json!({"when":empty,"apply":{"model":id,"y":angle}}));
                }
            }
        }
        result.blockstate = json!({"multipart":parts});
        return Some(result);
    } else if block == "snow" {
        for layers in 1..=8 {
            let id = model(
                &mut result,
                block,
                &format!("layers_{layers}"),
                vec![cuboid([0, 0, 0], [16, layers * 2, 16], "snow")],
            );
            variants.insert(format!("layers={layers}"), json!({"model":id}));
        }
    } else if block == "pale_hanging_moss" {
        for tip in [false, true] {
            let texture = if tip { "pale_hanging_moss_tip" } else { block };
            let id = model(
                &mut result,
                block,
                &format!("tip_{tip}"),
                cross(texture, false),
            );
            variants.insert(format!("tip={tip}"), json!({"model":id}));
        }
    } else if block == "vine" {
        let id = model(
            &mut result,
            block,
            "attached",
            vec![attached_plane(block, 16, true)],
        );
        let empty =
            json!({"east":"false","north":"false","south":"false","west":"false","up":"false"});
        let mut parts = Vec::new();
        for (facing, angle) in FACING {
            let apply = json!({"model":id,"y":angle});
            parts.push(json!({"when":{facing:"true"},"apply":apply}));
            parts.push(json!({"when":empty,"apply":apply}));
        }
        let apply = json!({"model":id,"x":270});
        parts.push(json!({"when":{"up":"true"},"apply":apply}));
        parts.push(json!({"when":empty,"apply":apply}));
        result.blockstate = json!({"multipart":parts});
        return Some(result);
    } else if block == "mangrove_roots" {
        // Original open lattice: crossed internal planes and cutout exterior,
        // with source-grounded side/top materials. Both wet states share shape.
        let mut exterior = cuboid([0, 0, 0], [16, 16, 16], "mangrove_roots_side");
        for face in ["up", "down"] {
            exterior["faces"][face]["texture"] = json!("minecraft:block/mangrove_roots_top");
        }
        let side = "minecraft:block/mangrove_roots_side";
        let inner_x = json!({"from":[8,0,0],"to":[8,16,16],"faces":{"east":{"texture":side},"west":{"texture":side}}});
        let inner_z = json!({"from":[0,0,8],"to":[16,16,8],"faces":{"north":{"texture":side},"south":{"texture":side}}});
        let id = model(
            &mut result,
            block,
            "lattice",
            vec![exterior, inner_x, inner_z],
        );
        variants.insert(String::new(), json!({"model":id}));
    } else if matches!(block, "mycelium" | "podzol") {
        let mut element = cuboid([0, 0, 0], [16, 16, 16], &format!("{block}_side"));
        element["faces"]["up"]["texture"] = json!(format!("minecraft:block/{block}_top"));
        element["faces"]["down"]["texture"] = json!("minecraft:block/dirt");
        let id = model(&mut result, block, "soil", vec![element]);
        variants.insert(String::new(), json!({"model":id}));
    } else if matches!(block, "melon" | "pale_moss_block" | "snow_block") {
        let texture = if block == "melon" {
            "melon_side"
        } else if block == "snow_block" {
            "snow"
        } else {
            block
        };
        let mut element = cuboid([0, 0, 0], [16, 16, 16], texture);
        if block == "melon" {
            for face in ["up", "down"] {
                element["faces"][face]["texture"] = json!("minecraft:block/melon_top");
            }
        }
        let id = model(&mut result, block, "solid", vec![element]);
        variants.insert(String::new(), json!({"model":id}));
    } else if matches!(
        block,
        "brown_mushroom_block" | "red_mushroom_block" | "mushroom_stem"
    ) {
        for mask in 0..64 {
            let mut element = cuboid([0, 0, 0], [16, 16, 16], block);
            let mut properties = Vec::new();
            for (index, face) in ["down", "up", "north", "south", "west", "east"]
                .into_iter()
                .enumerate()
            {
                let outside = mask & (1 << index) != 0;
                properties.push(format!("{face}={outside}"));
                if !outside {
                    element["faces"][face]["texture"] =
                        json!("minecraft:block/mushroom_block_inside");
                }
            }
            let id = model(&mut result, block, &format!("faces_{mask}"), vec![element]);
            variants.insert(properties.join(","), json!({"model":id}));
        }
    } else if block == "mangrove_propagule" {
        for age in 0..=4 {
            for hanging in [false, true] {
                let texture = if hanging {
                    "mangrove_propagule_hanging"
                } else {
                    block
                };
                let mut elements = cross(texture, false);
                if hanging {
                    for element in &mut elements {
                        element["from"][1] = json!(16 - (4 + 3 * age));
                    }
                }
                let id = model(
                    &mut result,
                    block,
                    &format!("age_{age}_hanging_{hanging}"),
                    elements,
                );
                variants.insert(format!("age={age},hanging={hanging}"), json!({"model":id}));
            }
        }
    } else if block == "shelf_mushroom" {
        for age in 0..=1 {
            let texture = format!("shelf_mushroom_stage{age}");
            let mut top = cuboid(
                [2 - 2 * age, 8 - age, 10 - 3 * age],
                [14 + 2 * age, 11, 16],
                &texture,
            );
            let mut base = cuboid(
                [5 - age, 6 - age, 12 - 2 * age],
                [11 + age, 8 - age, 16],
                &texture,
            );
            // Factual atlas rectangles from the pinned Java26.3 shelf models;
            // the two shelf volumes above are authored compatibility shapes.
            let rectangles = if age == 0 {
                [
                    [
                        [5., 3.5, 0., 7.],
                        [5., 3.5, 0., 0.],
                        [5., 2., 10., 3.],
                        [5., 3., 10., 4.],
                        [5., 1., 8.5, 2.],
                        [5., 0., 8.5, 1.],
                    ],
                    [
                        [3., 9., 0., 11.],
                        [3., 9., 0., 7.],
                        [5., 5., 8., 5.5],
                        [5., 5.5, 8., 6.],
                        [5., 4.5, 7., 5.],
                        [5., 4., 7., 4.5],
                    ],
                ]
            } else {
                [
                    [
                        [7., 5., 0., 10.],
                        [7., 5., 0., 0.],
                        [7., 3., 14., 4.5],
                        [7., 4.5, 14., 6.],
                        [7., 1.5, 12., 3.],
                        [7., 0., 12., 1.5],
                    ],
                    [
                        [4., 13., 0., 16.],
                        [4., 13., 0., 10.],
                        [4., 12., 8., 13.],
                        [4., 13., 8., 14.],
                        [4., 11., 7., 12.],
                        [4., 10., 7., 11.],
                    ],
                ]
            };
            for (element, rectangles) in [&mut top, &mut base].into_iter().zip(rectangles) {
                for (face, rectangle) in ["down", "up", "north", "south", "west", "east"]
                    .into_iter()
                    .zip(rectangles)
                {
                    element["faces"][face]["uv"] = json!(rectangle);
                }
            }
            let id = model(&mut result, block, &format!("age_{age}"), vec![top, base]);
            for (facing, angle) in FACING {
                variants.insert(
                    format!("age={age},facing={facing}"),
                    json!({"model":id,"y":angle}),
                );
            }
        }
    } else if block == "cocoa" {
        for age in 0..=2 {
            let size = 4 + 2 * age;
            let height = 5 + 2 * age;
            let texture = format!("cocoa_stage{age}");
            let mut pod = cuboid(
                [8 - size / 2, 12 - height, 1],
                [8 + size / 2, 12, 1 + size],
                &texture,
            );
            for face in ["up", "down"] {
                pod["faces"][face]["uv"] = json!([0, 0, size, size]);
            }
            for face in ["north", "south", "east", "west"] {
                pod["faces"][face]["uv"] = json!([16 - size, 4, 16, 4 + height]);
            }
            let stem = json!({"from":[8,12,0],"to":[8,16,4],"faces":{"east":{"texture":format!("minecraft:block/{texture}"),"uv":[12,0,16,4]},"west":{"texture":format!("minecraft:block/{texture}"),"uv":[16,0,12,4]}}});
            let id = model(&mut result, block, &format!("age_{age}"), vec![pod, stem]);
            for (facing, angle) in FACING {
                variants.insert(
                    format!("age={age},facing={facing}"),
                    json!({"model":id,"y":angle}),
                );
            }
        }
    } else if matches!(
        block,
        "sunflower"
            | "lilac"
            | "rose_bush"
            | "peony"
            | "tall_grass"
            | "large_fern"
            | "pitcher_plant"
    ) {
        for half in ["lower", "upper"] {
            let suffix = if half == "lower" { "bottom" } else { "top" };
            let tinted = matches!(block, "tall_grass" | "large_fern");
            let texture = if block == "pitcher_plant" {
                format!("pitcher_crop_{suffix}_stage_4")
            } else {
                format!("{block}_{suffix}")
            };
            let mut elements = cross(&texture, tinted);
            if block == "sunflower" && half == "upper" {
                // Sunflower fronts face east; retain authored geometry while
                // using the documented tilted-head direction and source art.
                elements.push(json!({"from":[9,0,1],"to":[9,16,15],"shade":false,"rotation":{"origin":[8,8,8],"axis":"z","angle":22.5,"rescale":true},"faces":{"east":{"texture":"minecraft:block/sunflower_front","uv":[0,0,16,16]},"west":{"texture":"minecraft:block/sunflower_back","uv":[0,0,16,16]}}}));
            }
            let id = model(&mut result, block, half, elements);
            variants.insert(format!("half={half}"), json!({"model":id}));
        }
    } else if block == "bamboo" {
        for age in [0, 1] {
            for leaves in ["none", "small", "large"] {
                let last = if age == 0 { 9 } else { 10 };
                let mut elements = vec![cuboid([7, 0, 7], [last, 16, last], "bamboo_stalk")];
                if leaves != "none" {
                    elements.extend(cross(&format!("bamboo_{leaves}_leaves"), true));
                }
                let id = model(&mut result, block, &format!("{age}_{leaves}"), elements);
                variants.insert(format!("age={age},leaves={leaves}"), json!({"model":id}));
            }
        }
    } else if matches!(block, "bamboo_block" | "stripped_bamboo_block") {
        let mut element = cuboid([0, 0, 0], [16, 16, 16], block);
        for face in ["up", "down"] {
            element["faces"][face]["texture"] = json!(format!("minecraft:block/{block}_top"));
        }
        let id = model(&mut result, block, "column", vec![element]);
        for (axis, x, y) in [("y", 0, 0), ("x", 90, 90), ("z", 90, 0)] {
            variants.insert(format!("axis={axis}"), json!({"model":id,"x":x,"y":y}));
        }
    } else if let Some(prefix) = block.strip_suffix("_wood") {
        let species = prefix.strip_prefix("stripped_").unwrap_or(prefix);
        if !WOODS.contains(&species) || species == "bamboo" {
            return None;
        }
        let id = model(
            &mut result,
            block,
            "bark",
            vec![cuboid([0, 0, 0], [16, 16, 16], &format!("{prefix}_log"))],
        );
        for (axis, x, y) in [("y", 0, 0), ("x", 90, 90), ("z", 90, 0)] {
            variants.insert(format!("axis={axis}"), json!({"model":id,"x":x,"y":y}));
        }
    } else if let Some(color) = block.strip_suffix("_bed") {
        if ![
            "white",
            "orange",
            "magenta",
            "light_blue",
            "yellow",
            "lime",
            "pink",
            "gray",
            "light_gray",
            "cyan",
            "purple",
            "blue",
            "brown",
            "green",
            "red",
            "black",
        ]
        .contains(&color)
        {
            return None;
        }
        for part in ["head", "foot"] {
            let foot = part == "foot";
            let (first_z, last_z) = if foot { (13, 16) } else { (0, 3) };
            let elements = vec![
                bed_cuboid([0, 3, 0], [16, 9, 16], color, false, foot),
                bed_cuboid([0, 0, first_z], [3, 3, last_z], color, true, foot),
                bed_cuboid([13, 0, first_z], [16, 3, last_z], color, true, foot),
            ];
            let id = model(&mut result, block, part, elements);
            for (facing, angle) in FACING {
                variants.insert(
                    format!("facing={facing},part={part}"),
                    json!({"model":id,"y":angle}),
                );
            }
        }
    } else if matches!(block, "pumpkin" | "carved_pumpkin" | "jack_o_lantern") {
        let mut element = cuboid([0, 0, 0], [16, 16, 16], "pumpkin_side");
        for face in ["up", "down"] {
            element["faces"][face]["texture"] = json!("minecraft:block/pumpkin_top");
        }
        if block != "pumpkin" {
            element["faces"]["north"]["texture"] = json!(format!("minecraft:block/{block}"));
        }
        let id = model(&mut result, block, "faces", vec![element]);
        if block == "pumpkin" {
            variants.insert(String::new(), json!({"model":id}));
        } else {
            for (facing, angle) in FACING {
                variants.insert(format!("facing={facing}"), json!({"model":id,"y":angle}));
            }
        }
    } else if matches!(block, "beehive" | "bee_nest") {
        for honey in 0..=5 {
            let mut element = cuboid([0, 0, 0], [16, 16, 16], &format!("{block}_side"));
            let top = if block == "beehive" {
                "beehive_end"
            } else {
                "bee_nest_top"
            };
            let bottom = if block == "beehive" {
                "beehive_end"
            } else {
                "bee_nest_bottom"
            };
            element["faces"]["up"]["texture"] = json!(format!("minecraft:block/{top}"));
            element["faces"]["down"]["texture"] = json!(format!("minecraft:block/{bottom}"));
            element["faces"]["north"]["texture"] = json!(format!(
                "minecraft:block/{block}_front{}",
                if honey == 5 { "_honey" } else { "" }
            ));
            let id = model(&mut result, block, &format!("honey_{honey}"), vec![element]);
            for (facing, angle) in FACING {
                variants.insert(
                    format!("facing={facing},honey_level={honey}"),
                    json!({"model":id,"y":angle}),
                );
            }
        }
    } else if let Some(prefix) = block.strip_suffix("_fence_gate") {
        if !["oak", "jungle", "acacia", "spruce"].contains(&prefix) {
            return None;
        }
        let texture = material(prefix)?;
        for in_wall in [false, true] {
            let top = if in_wall { 13 } else { 16 };
            let shift = if in_wall { 3 } else { 0 };
            for open in [false, true] {
                let mut elements = vec![
                    cuboid([0, 0, 7], [2, top, 9], &texture),
                    cuboid([14, 0, 7], [16, top, 9], &texture),
                ];
                if open {
                    for (left, right) in [(0, 2), (14, 16)] {
                        elements.push(cuboid(
                            [left, 4 - shift, 9],
                            [right, 6 - shift, 15],
                            &texture,
                        ));
                        elements.push(cuboid(
                            [left, 10 - shift, 9],
                            [right, 12 - shift, 15],
                            &texture,
                        ));
                    }
                } else {
                    elements.push(cuboid([2, 4 - shift, 7], [14, 6 - shift, 9], &texture));
                    elements.push(cuboid([2, 10 - shift, 7], [14, 12 - shift, 9], &texture));
                }
                let id = model(
                    &mut result,
                    block,
                    &format!("wall_{in_wall}_open_{open}"),
                    elements,
                );
                for (facing, angle) in FACING {
                    variants.insert(
                        format!("facing={facing},in_wall={in_wall},open={open}"),
                        json!({"model":id,"y":angle}),
                    );
                }
            }
        }
    } else if let Some(prefix) = block.strip_suffix("_fence") {
        if !WOODS.contains(&prefix) {
            return None;
        }
        let texture = material(prefix)?;
        let post = model(
            &mut result,
            block,
            "post",
            vec![cuboid([6, 0, 6], [10, 16, 10], &texture)],
        );
        let rail = model(
            &mut result,
            block,
            "rail",
            vec![
                cuboid([7, 6, 0], [9, 9, 6], &texture),
                cuboid([7, 12, 0], [9, 15, 6], &texture),
            ],
        );
        let mut parts = vec![json!({"apply":{"model":post}})];
        for (facing, angle) in FACING {
            let condition: Map<String, Value> =
                [(facing.into(), json!("true"))].into_iter().collect();
            parts.push(json!({"when":condition,"apply":{"model":rail,"y":angle,"uvlock":true}}));
        }
        result.blockstate = json!({"multipart":parts});
        return Some(result);
    } else if let Some(prefix) = block.strip_suffix("_wall") {
        if ![
            "cobblestone",
            "mossy_cobblestone",
            "stone_brick",
            "mossy_stone_brick",
            "brick",
            "sandstone",
            "red_sandstone",
            "granite",
            "diorite",
            "andesite",
            "cobbled_deepslate",
            "polished_deepslate",
            "deepslate_brick",
            "deepslate_tile",
            "tuff",
            "polished_tuff",
            "tuff_brick",
        ]
        .contains(&prefix)
        {
            return None;
        }
        let texture = material(prefix)?;
        let post = model(
            &mut result,
            block,
            "post",
            vec![cuboid([4, 0, 4], [12, 16, 12], &texture)],
        );
        let mut parts = vec![json!({"when":{"up":"true"},"apply":{"model":post}})];
        for (height, top) in [("low", 14), ("tall", 16)] {
            let rail = model(
                &mut result,
                block,
                height,
                vec![cuboid([5, 0, 0], [11, top, 8], &texture)],
            );
            for (facing, angle) in FACING {
                let condition: Map<String, Value> =
                    [(facing.into(), json!(height))].into_iter().collect();
                parts
                    .push(json!({"when":condition,"apply":{"model":rail,"y":angle,"uvlock":true}}));
            }
        }
        result.blockstate = json!({"multipart":parts});
        return Some(result);
    } else if let Some(prefix) = block.strip_suffix("_trapdoor") {
        if !WOODS.contains(&prefix) && prefix != "iron" {
            return None;
        }
        for half in ["bottom", "top"] {
            for open in [false, true] {
                let (from, to) = if open {
                    ([0, 0, 13], [16, 16, 16])
                } else if half == "bottom" {
                    ([0, 0, 0], [16, 3, 16])
                } else {
                    ([0, 13, 0], [16, 16, 16])
                };
                let id = model(
                    &mut result,
                    block,
                    &format!("{half}_{open}"),
                    vec![cuboid(from, to, block)],
                );
                for (facing, angle) in FACING {
                    variants.insert(
                        format!("facing={facing},half={half},open={open}"),
                        json!({"model":id,"y":angle}),
                    );
                }
            }
        }
    } else if let Some(prefix) = block.strip_suffix("_slab") {
        let texture = material(prefix)?;
        for (state, bottom, top) in [("bottom", 0, 8), ("top", 8, 16), ("double", 0, 16)] {
            let id = model(
                &mut result,
                block,
                state,
                vec![cuboid([0, bottom, 0], [16, top, 16], &texture)],
            );
            variants.insert(format!("type={state}"), json!({"model":id}));
        }
    } else if let Some(prefix) = block.strip_suffix("_stairs") {
        if matches!(prefix, "cut_sandstone" | "cut_red_sandstone") {
            return None;
        }
        let texture = material(prefix)?;
        for half in ["bottom", "top"] {
            let (base_min, base_max, step_min, step_max) = if half == "bottom" {
                (0, 8, 8, 16)
            } else {
                (8, 16, 0, 8)
            };
            for shape in [
                "straight",
                "inner_left",
                "inner_right",
                "outer_left",
                "outer_right",
            ] {
                let mut elements = vec![cuboid([0, base_min, 0], [16, base_max, 16], &texture)];
                let (left, right) = match shape {
                    "outer_left" => (0, 8),
                    "outer_right" => (8, 16),
                    _ => (0, 16),
                };
                elements.push(cuboid([left, step_min, 0], [right, step_max, 8], &texture));
                match shape {
                    "inner_left" => {
                        elements.push(cuboid([0, step_min, 8], [8, step_max, 16], &texture))
                    }
                    "inner_right" => {
                        elements.push(cuboid([8, step_min, 8], [16, step_max, 16], &texture))
                    }
                    _ => {}
                }
                let id = model(&mut result, block, &format!("{half}_{shape}"), elements);
                for (facing, angle) in FACING {
                    variants.insert(
                        format!("facing={facing},half={half},shape={shape}"),
                        json!({"model":id,"y":angle,"uvlock":true}),
                    );
                }
            }
        }
    } else if let Some(prefix) = block.strip_suffix("_door") {
        if !WOODS.contains(&prefix) && prefix != "iron" {
            return None;
        }
        for half in ["lower", "upper"] {
            let texture = format!("{block}_{}", if half == "lower" { "bottom" } else { "top" });
            for hinge in ["left", "right"] {
                for open in [false, true] {
                    let (from, to) = match (open, hinge) {
                        (false, _) => ([0, 0, 13], [16, 16, 16]),
                        (true, "left") => ([0, 0, 0], [3, 16, 16]),
                        _ => ([13, 0, 0], [16, 16, 16]),
                    };
                    let id = model(
                        &mut result,
                        block,
                        &format!("{half}_{hinge}_{open}"),
                        vec![cuboid(from, to, &texture)],
                    );
                    for (facing, angle) in FACING {
                        variants.insert(
                            format!("facing={facing},half={half},hinge={hinge},open={open}"),
                            json!({"model":id,"y":angle}),
                        );
                    }
                }
            }
        }
    } else {
        return None;
    }
    result.blockstate = json!({"variants":variants});
    Some(result)
}

#[cfg(test)]
mod tests {

    fn compile_cultivated_plant_quads(
        id: &str,
    ) -> Vec<crate::scenes::voxel_landscape::assets::models::ModelQuad> {
        use crate::scenes::voxel_landscape::assets::{
            block_state::BlockState,
            budget::{ByteBudget, Cancel, Limits},
            compatibility::DefinitionSet,
            identity::{AssetPath, BlobOrigin, Digest256, Label, OriginKind, ResourceId},
            layers::{ResourceKey, ResourceKind},
            models::ModelCompiler,
        };
        use std::sync::atomic::AtomicBool;
        let budget = ByteBudget::new(16 << 20).unwrap();
        let stop = AtomicBool::new(false);
        let cancel = Cancel::new(&stop);
        let origin = BlobOrigin {
            pack: ResourceId::parse("ilium:plant_geometry_fixture").unwrap(),
            release: Label::new("Synthetic plant geometry regression").unwrap(),
            layer: Label::new("Original compatibility geometry").unwrap(),
            path: AssetPath::parse("plant_fixture.json").unwrap(),
            review_digest: Digest256::of(b"Synthetic plant geometry fixture; no source artwork"),
            kind: OriginKind::DiagnosticFixture,
        };
        let mut provider = DefinitionSet::new(origin, Limits::default(), budget.clone()).unwrap();
        let geometry =
            definitions(id).expect("Cultivated plant must have explicit plant-shaped geometry");
        let reason = Label::new("Synthetic compiled plant shape regression").unwrap();
        for (name, value) in geometry.models {
            provider
                .insert_model(&name, &value, reason.clone(), cancel)
                .unwrap();
        }
        let resource = ResourceId::parse(id).unwrap();
        provider
            .insert(
                ResourceKey {
                    kind: ResourceKind::Blockstate,
                    id: resource.clone(),
                },
                &geometry.blockstate,
                reason,
                cancel,
            )
            .unwrap();
        let state = BlockState::new(resource, Vec::new()).unwrap();
        let mut compiler = ModelCompiler::new(&provider, Limits::default(), budget).unwrap();
        let normalized = compiler
            .compile_state(&state, [0, 0, 64], 0, cancel)
            .unwrap();
        normalized
            .applications
            .iter()
            .flat_map(|(_, model)| model.quads.clone())
            .collect()
    }
    #[test]
    fn cultivated_small_flowers_compile_as_rooted_crosses() {
        for id in [
            "minecraft:torchflower",
            "minecraft:golden_dandelion",
            "minecraft:wither_rose",
        ] {
            let quads = compile_cultivated_plant_quads(id);
            assert!(!quads.is_empty());
            assert!(
                quads.iter().all(|q| q.normal[2].abs() < 0.001
                    && q.normal[0].abs() > 0.6
                    && q.normal[1].abs() > 0.6
                    && q.cull_face.is_none()),
                "{id} produced box faces rather than standing flower sheets"
            );
            assert!(
                quads.iter().all(|q| !q.shade
                    && q.texture.as_str() == id.replace("minecraft:", "minecraft:block/")),
                "{id} lost its own sprite or added cube-face darkening"
            );
            let heights: Vec<_> = quads
                .iter()
                .flat_map(|q| q.points.iter().map(|p| p[2]))
                .collect();
            assert!(heights.iter().any(|z| z.abs() < 0.001));
            assert!(heights.iter().all(|z| (0.0..=1.0).contains(z)));
        }
    }
    #[test]
    fn cultivated_azalea_shrubs_compile_leaf_heads_and_rooted_stems() {
        for id in ["minecraft:azalea", "minecraft:flowering_azalea"] {
            let quads = compile_cultivated_plant_quads(id);
            assert!(
                quads.iter().any(|q| q.normal[2] > 0.9
                    && q.texture.as_str()
                        == format!(
                            "minecraft:block/{}_top",
                            id.trim_start_matches("minecraft:")
                        )),
                "{id} needs a textured leaf head"
            );
            let stems: Vec<_> = quads
                .iter()
                .filter(|q| q.texture.as_str() == "minecraft:block/azalea_plant")
                .collect();
            assert!(!stems.is_empty(), "{id} lost its rooted stem");
            assert!(stems
                .iter()
                .any(|q| q.points.iter().any(|p| p[2].abs() < 0.001)));
            assert!(
                stems
                    .iter()
                    .all(|q| q.points.iter().all(|p| p[2] >= 0.0 && p[2] < 0.5)),
                "{id} stem must stay below the leafy head"
            );
            assert!(quads
                .iter()
                .all(|q| q.points.iter().all(|p| (0.0..=1.0).contains(&p[2]))));
        }
    }
    #[test]
    fn fletching_workbench_and_target_use_real_face_materials() {
        let bench = definitions("minecraft:fletching_table").unwrap();
        let id = bench.blockstate["variants"][""]["model"].as_str().unwrap();
        let face = &bench.models[id]["elements"][0]["faces"];
        for (side, material) in [
            ("up", "fletching_table_top"),
            ("down", "birch_planks"),
            ("north", "fletching_table_front"),
            ("south", "fletching_table_front"),
            ("east", "fletching_table_side"),
            ("west", "fletching_table_side"),
        ] {
            assert_eq!(face[side]["texture"], format!("minecraft:block/{material}"));
        }
        let target = definitions("minecraft:target").unwrap();
        let id = target.blockstate["variants"][""]["model"].as_str().unwrap();
        let face = &target.models[id]["elements"][0]["faces"];
        for side in ["up", "down"] {
            assert_eq!(face[side]["texture"], "minecraft:block/target_top");
        }
        for side in ["north", "south", "east", "west"] {
            assert_eq!(face[side]["texture"], "minecraft:block/target_side");
        }
    }

    #[test]
    fn structure_materials_use_real_distinct_faces_and_crop_stages() {
        for block in ["hay_block", "bone_block"] {
            let g = definitions(&format!("minecraft:{block}")).unwrap();
            assert_eq!(g.blockstate["variants"].as_object().unwrap().len(), 3);
            let id = g.blockstate["variants"]["axis=y"]["model"]
                .as_str()
                .unwrap();
            let faces = &g.models[id]["elements"][0]["faces"];
            assert_eq!(
                faces["up"]["texture"],
                format!("minecraft:block/{block}_top")
            );
            assert_eq!(faces["down"]["texture"], faces["up"]["texture"]);
            assert_eq!(
                faces["east"]["texture"],
                format!("minecraft:block/{block}_side")
            );
        }
        let path = definitions("minecraft:dirt_path").unwrap();
        let id = path.blockstate["variants"][""]["model"].as_str().unwrap();
        let element = &path.models[id]["elements"][0];
        assert_eq!(element["to"], json!([16, 15, 16]));
        assert_eq!(
            element["faces"]["up"]["texture"],
            "minecraft:block/dirt_path_top"
        );
        assert_eq!(element["faces"]["down"]["texture"], "minecraft:block/dirt");
        assert_eq!(
            element["faces"]["east"]["texture"],
            "minecraft:block/dirt_path_side"
        );
        let wheat = definitions("minecraft:wheat").unwrap();
        for age in 0..=7 {
            let id = wheat.blockstate["variants"][format!("age={age}")]["model"]
                .as_str()
                .unwrap();
            for element in wheat.models[id]["elements"].as_array().unwrap() {
                assert_eq!(
                    element["faces"]["north"]["texture"],
                    format!("minecraft:block/wheat_stage{age}")
                );
            }
        }
        let barrel = definitions("minecraft:barrel").unwrap();
        assert_eq!(barrel.blockstate["variants"].as_object().unwrap().len(), 12);
        for (open, top) in [(false, "barrel_top"), (true, "barrel_top_open")] {
            let id = barrel.blockstate["variants"][format!("facing=up,open={open}")]["model"]
                .as_str()
                .unwrap();
            let faces = &barrel.models[id]["elements"][0]["faces"];
            assert_eq!(faces["up"]["texture"], format!("minecraft:block/{top}"));
            assert_eq!(faces["down"]["texture"], "minecraft:block/barrel_bottom");
            assert_eq!(faces["east"]["texture"], "minecraft:block/barrel_side");
        }
        let furnace = definitions("minecraft:furnace").unwrap();
        assert_eq!(furnace.blockstate["variants"].as_object().unwrap().len(), 8);
        for (lit, front) in [(false, "furnace_front"), (true, "furnace_front_on")] {
            let id = furnace.blockstate["variants"][format!("facing=north,lit={lit}")]["model"]
                .as_str()
                .unwrap();
            assert_eq!(
                furnace.models[id]["elements"][0]["faces"]["north"]["texture"],
                format!("minecraft:block/{front}")
            );
        }
        let table = definitions("minecraft:crafting_table").unwrap();
        let id = table.blockstate["variants"][""]["model"].as_str().unwrap();
        let faces = &table.models[id]["elements"][0]["faces"];
        assert_eq!(faces["up"]["texture"], "minecraft:block/crafting_table_top");
        assert_eq!(faces["down"]["texture"], "minecraft:block/oak_planks");
        assert_eq!(
            faces["east"]["texture"],
            "minecraft:block/crafting_table_side"
        );
        assert_eq!(
            faces["north"]["texture"],
            "minecraft:block/crafting_table_front"
        );
    }

    #[test]
    fn crop_stems_have_age_geometry_instead_of_trunk_axis() {
        for crop in ["melon_stem", "pumpkin_stem"] {
            let geometry = definitions(&format!("minecraft:{crop}")).unwrap();
            assert_eq!(
                geometry.blockstate["variants"].as_object().unwrap().len(),
                8
            );
            for age in 0..=7 {
                let key = format!("age={age}");
                let id = geometry.blockstate["variants"][key]["model"]
                    .as_str()
                    .unwrap();
                let elements = geometry.models[id]["elements"].as_array().unwrap();
                assert_eq!(elements.len(), 2);
                for element in elements {
                    assert_eq!(element["from"][1], 0);
                    assert_eq!(element["to"][1], (age + 1) * 2);
                    for face in ["north", "south"] {
                        assert_eq!(
                            element["faces"][face]["texture"],
                            format!("minecraft:block/{crop}")
                        );
                        assert_eq!(element["faces"][face]["tintindex"], 0);
                        assert_eq!(
                            element["faces"][face]["uv"],
                            json!([0, 16 - (age + 1) * 2, 16, 16])
                        );
                    }
                }
            }
        }
    }

    use super::*;
    #[test]
    fn slab_has_three_distinct_state_shapes_and_real_plank_material() {
        let result = definitions("minecraft:oak_slab").unwrap();
        let variants = result.blockstate["variants"].as_object().unwrap();
        assert_eq!(variants.len(), 3);
        let bottom = variants["type=bottom"]["model"].as_str().unwrap();
        let top = variants["type=top"]["model"].as_str().unwrap();
        let double = variants["type=double"]["model"].as_str().unwrap();
        assert_eq!(result.models[bottom]["elements"][0]["to"][1], 8);
        assert_eq!(result.models[top]["elements"][0]["from"][1], 8);
        assert_eq!(result.models[double]["elements"][0]["to"][1], 16);
        assert_eq!(
            result.models[bottom]["elements"][0]["faces"]["up"]["texture"],
            "minecraft:block/oak_planks"
        );
    }
    #[test]
    fn doors_cover_half_hinge_open_and_facing_without_cube_geometry() {
        let result = definitions("minecraft:dark_oak_door").unwrap();
        let variants = result.blockstate["variants"].as_object().unwrap();
        assert_eq!(variants.len(), 32);
        for model in result.models.values() {
            let element = &model["elements"][0];
            let from = element["from"].as_array().unwrap();
            let to = element["to"].as_array().unwrap();
            assert!(
                (0..3).any(|axis| to[axis].as_i64().unwrap() - from[axis].as_i64().unwrap() == 3)
            );
        }
        let upper = variants["facing=north,half=upper,hinge=left,open=false"]["model"]
            .as_str()
            .unwrap();
        assert_eq!(
            result.models[upper]["elements"][0]["faces"]["north"]["texture"],
            "minecraft:block/dark_oak_door_top"
        );
    }
    #[test]
    fn stairs_cover_all_forty_direction_half_corner_states() {
        let result = definitions("minecraft:stone_brick_stairs").unwrap();
        assert_eq!(result.blockstate["variants"].as_object().unwrap().len(), 40);
        assert!(result
            .models
            .values()
            .all(|model| model["elements"].as_array().unwrap().len() >= 2));
    }
    #[test]
    fn unrelated_texture_filename_resemblance_is_not_material_evidence() {
        assert!(definitions("minecraft:unknown_slab").is_none());
        assert!(definitions("other:oak_slab").is_none());
        assert!(definitions("minecraft:oak_wall").is_none());
        assert!(definitions("minecraft:stone_wall").is_none());
        assert!(definitions("minecraft:cut_sandstone_stairs").is_none());
    }
    #[test]
    fn fence_connections_and_wall_heights_are_independent_parts() {
        let fence = definitions("minecraft:oak_fence").unwrap();
        assert_eq!(fence.blockstate["multipart"].as_array().unwrap().len(), 5);
        let wall = definitions("minecraft:cobblestone_wall").unwrap();
        assert_eq!(wall.blockstate["multipart"].as_array().unwrap().len(), 9);
    }
    #[test]
    fn hive_honey_front_and_pumpkin_top_have_distinct_source_textures() {
        let hive = definitions("minecraft:bee_nest").unwrap();
        let key = "facing=north,honey_level=5";
        let id = hive.blockstate["variants"][key]["model"].as_str().unwrap();
        let faces = &hive.models[id]["elements"][0]["faces"];
        assert_eq!(
            faces["north"]["texture"],
            "minecraft:block/bee_nest_front_honey"
        );
        assert_eq!(faces["up"]["texture"], "minecraft:block/bee_nest_top");
        let pumpkin = definitions("minecraft:pumpkin").unwrap();
        let id = pumpkin.blockstate["variants"][""]["model"]
            .as_str()
            .unwrap();
        let faces = &pumpkin.models[id]["elements"][0]["faces"];
        assert_eq!(faces["up"]["texture"], "minecraft:block/pumpkin_top");
        assert_eq!(faces["north"]["texture"], "minecraft:block/pumpkin_side");
    }
    #[test]
    fn trapdoors_change_plane_with_half_and_open_state() {
        let result = definitions("minecraft:oak_trapdoor").unwrap();
        assert_eq!(result.blockstate["variants"].as_object().unwrap().len(), 16);
        let id = result.blockstate["variants"]["facing=north,half=bottom,open=true"]["model"]
            .as_str()
            .unwrap();
        assert_eq!(result.models[id]["elements"][0]["from"][2], 13);
    }
    #[test]
    fn beds_use_the_real_entity_atlas_and_distinct_head_foot_rectangles() {
        let bed = definitions("minecraft:yellow_bed").unwrap();
        assert_eq!(bed.blockstate["variants"].as_object().unwrap().len(), 8);
        let head = bed.blockstate["variants"]["facing=north,part=head"]["model"]
            .as_str()
            .unwrap();
        let foot = bed.blockstate["variants"]["facing=north,part=foot"]["model"]
            .as_str()
            .unwrap();
        let top = &bed.models[head]["elements"][0]["faces"]["up"];
        assert_eq!(top["texture"], "minecraft:entity/bed/yellow");
        assert_ne!(
            top["uv"],
            bed.models[foot]["elements"][0]["faces"]["up"]["uv"]
        );
        assert_eq!(bed.models[head]["elements"][0]["to"][1], 9);
        assert_eq!(bed.models[head]["elements"].as_array().unwrap().len(), 3);
        assert!(definitions("minecraft:invented_bed").is_none());
    }
    #[test]
    fn berry_growth_uses_each_real_stage_image_without_invented_texture() {
        let geometry = definitions("minecraft:sweet_berry_bush").unwrap();
        assert_eq!(
            geometry.blockstate["variants"].as_object().unwrap().len(),
            4
        );
        for age in 0..=3 {
            let id = geometry.blockstate["variants"][format!("age={age}")]["model"]
                .as_str()
                .unwrap();
            let elements = geometry.models[id]["elements"].as_array().unwrap();
            assert_eq!(elements.len(), 2);
            for element in elements {
                for face in element["faces"].as_object().unwrap().values() {
                    assert_eq!(
                        face["texture"],
                        format!("minecraft:block/sweet_berry_bush_stage{age}")
                    );
                }
            }
        }
    }
    #[test]
    fn double_plants_bind_distinct_real_upper_and_lower_images() {
        let plant = definitions("minecraft:rose_bush").unwrap();
        let lower = plant.blockstate["variants"]["half=lower"]["model"]
            .as_str()
            .unwrap();
        let upper = plant.blockstate["variants"]["half=upper"]["model"]
            .as_str()
            .unwrap();
        assert_eq!(
            plant.models[lower]["elements"][0]["faces"]["north"]["texture"],
            "minecraft:block/rose_bush_bottom"
        );
        assert_eq!(
            plant.models[upper]["elements"][0]["faces"]["north"]["texture"],
            "minecraft:block/rose_bush_top"
        );
        let sunflower = definitions("minecraft:sunflower").unwrap();
        let upper = sunflower.blockstate["variants"]["half=upper"]["model"]
            .as_str()
            .unwrap();
        assert_eq!(
            sunflower.models[upper]["elements"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
    }
    #[test]
    fn wood_bark_uses_log_images_on_all_faces_including_cut_axes() {
        let wood = definitions("minecraft:stripped_oak_wood").unwrap();
        let id = wood.blockstate["variants"]["axis=y"]["model"]
            .as_str()
            .unwrap();
        assert!(wood.models[id]["elements"][0]["faces"]
            .as_object()
            .unwrap()
            .values()
            .all(|face| face["texture"] == "minecraft:block/stripped_oak_log"));
        assert!(definitions("minecraft:bamboo_wood").is_none());
    }
    #[test]
    fn bamboo_has_stalk_geometry_and_separate_leaf_state_artwork() {
        let bamboo = definitions("minecraft:bamboo").unwrap();
        assert_eq!(bamboo.blockstate["variants"].as_object().unwrap().len(), 6);
        let id = bamboo.blockstate["variants"]["age=1,leaves=large"]["model"]
            .as_str()
            .unwrap();
        assert_eq!(bamboo.models[id]["elements"].as_array().unwrap().len(), 3);
        assert_eq!(
            bamboo.models[id]["elements"][0]["faces"]["up"]["texture"],
            "minecraft:block/bamboo_stalk"
        );
    }
    #[test]
    fn bamboo_blocks_keep_distinct_end_textures_on_each_axis() {
        for block in ["bamboo_block", "stripped_bamboo_block"] {
            let geometry = definitions(&format!("minecraft:{block}")).unwrap();
            assert_eq!(
                geometry.blockstate["variants"].as_object().unwrap().len(),
                3
            );
            let id = geometry.blockstate["variants"]["axis=y"]["model"]
                .as_str()
                .unwrap();
            let faces = &geometry.models[id]["elements"][0]["faces"];
            assert_eq!(
                faces["up"]["texture"],
                format!("minecraft:block/{block}_top")
            );
            assert_eq!(faces["down"]["texture"], faces["up"]["texture"]);
            assert_eq!(
                faces["north"]["texture"],
                format!("minecraft:block/{block}")
            );
        }
    }
    #[test]
    fn sunflower_front_faces_east_on_a_tilted_head() {
        let geometry = definitions("minecraft:sunflower").unwrap();
        let id = geometry.blockstate["variants"]["half=upper"]["model"]
            .as_str()
            .unwrap();
        let head = &geometry.models[id]["elements"][2];
        assert_eq!(
            head["faces"]["east"]["texture"],
            "minecraft:block/sunflower_front"
        );
        assert_eq!(
            head["faces"]["west"]["texture"],
            "minecraft:block/sunflower_back"
        );
        assert_eq!(head["rotation"]["axis"], "z");
        assert_eq!(head["rotation"]["angle"], 22.5);
    }
    #[test]
    fn mature_pitcher_plant_uses_actual_crop_stage_four_images() {
        let geometry = definitions("minecraft:pitcher_plant").unwrap();
        for (half, suffix) in [("lower", "bottom"), ("upper", "top")] {
            let id = geometry.blockstate["variants"][format!("half={half}")]["model"]
                .as_str()
                .unwrap();
            assert_eq!(
                geometry.models[id]["elements"][0]["faces"]["north"]["texture"],
                format!("minecraft:block/pitcher_crop_{suffix}_stage_4")
            );
        }
    }
    #[test]
    fn cactus_uses_inset_sides_and_real_top_bottom_materials() {
        let geometry = definitions("minecraft:cactus").unwrap();
        let id = geometry.blockstate["variants"][""]["model"]
            .as_str()
            .unwrap();
        let element = &geometry.models[id]["elements"][0];
        assert_eq!(element["from"], json!([1, 0, 1]));
        assert_eq!(element["to"], json!([15, 16, 15]));
        assert_eq!(
            element["faces"]["up"]["texture"],
            "minecraft:block/cactus_top"
        );
        assert_eq!(
            element["faces"]["down"]["texture"],
            "minecraft:block/cactus_bottom"
        );
        assert_eq!(
            element["faces"]["east"]["texture"],
            "minecraft:block/cactus_side"
        );
    }
    #[test]
    fn ground_covers_are_thin_and_use_distinct_amount_states() {
        for block in ["leaf_litter", "pink_petals", "wildflowers"] {
            let geometry = definitions(&format!("minecraft:{block}")).unwrap();
            assert_eq!(
                geometry.blockstate["variants"].as_object().unwrap().len(),
                16
            );
            for value in geometry.models.values() {
                for element in value["elements"].as_array().unwrap() {
                    assert!(element["to"][1].as_f64().unwrap() <= 3.0);
                }
            }
        }
        for block in ["lily_pad", "moss_carpet"] {
            let geometry = definitions(&format!("minecraft:{block}")).unwrap();
            for value in geometry.models.values() {
                assert!(value["elements"][0]["to"][1].as_f64().unwrap() <= 1.0);
            }
        }
    }
    #[test]
    fn snow_layers_and_moss_tips_keep_their_geometry_state() {
        let snow = definitions("minecraft:snow").unwrap();
        for layers in 1..=8 {
            let id = snow.blockstate["variants"][format!("layers={layers}")]["model"]
                .as_str()
                .unwrap();
            assert_eq!(snow.models[id]["elements"][0]["to"][1], layers * 2);
        }
        let moss = definitions("minecraft:pale_hanging_moss").unwrap();
        let id = moss.blockstate["variants"]["tip=true"]["model"]
            .as_str()
            .unwrap();
        assert_eq!(
            moss.models[id]["elements"][0]["faces"]["north"]["texture"],
            "minecraft:block/pale_hanging_moss_tip"
        );
        assert!(definitions("minecraft:pale_moss_carpet")
            .unwrap()
            .blockstate["multipart"]
            .is_array());
    }
    #[test]
    fn cocoa_age_and_attached_vine_keep_material_and_direction() {
        let cocoa = definitions("minecraft:cocoa").unwrap();
        assert_eq!(cocoa.blockstate["variants"].as_object().unwrap().len(), 12);
        for age in 0..=2 {
            let id = cocoa.blockstate["variants"][format!("age={age},facing=north")]["model"]
                .as_str()
                .unwrap();
            assert_eq!(
                cocoa.models[id]["elements"][0]["faces"]["up"]["texture"],
                format!("minecraft:block/cocoa_stage{age}")
            );
        }
        let vine = definitions("minecraft:vine").unwrap();
        assert_eq!(vine.blockstate["multipart"].as_array().unwrap().len(), 10);
        for value in vine.models.values() {
            assert_eq!(
                value["elements"][0]["from"][2],
                value["elements"][0]["to"][2]
            );
            assert_eq!(value["elements"][0]["faces"]["north"]["tintindex"], 0);
        }
    }
    #[test]
    fn poplar_wood_and_solid_flora_use_materials_and_full_volume() {
        for block in [
            "poplar_wood",
            "stripped_poplar_wood",
            "poplar_stairs",
            "poplar_door",
        ] {
            assert!(definitions(&format!("minecraft:{block}")).is_some());
        }
        for block in [
            "melon",
            "brown_mushroom_block",
            "red_mushroom_block",
            "mushroom_stem",
            "pale_moss_block",
        ] {
            let geometry = definitions(&format!("minecraft:{block}")).unwrap();
            for value in geometry.models.values() {
                assert_eq!(value["elements"][0]["to"], json!([16, 16, 16]));
            }
        }
        let melon = definitions("minecraft:melon").unwrap();
        let model = melon.models.values().next().unwrap();
        assert_eq!(
            model["elements"][0]["faces"]["up"]["texture"],
            "minecraft:block/melon_top"
        );
        assert_eq!(
            model["elements"][0]["faces"]["north"]["texture"],
            "minecraft:block/melon_side"
        );
    }
    #[test]
    fn hanging_propagules_and_shelf_mushrooms_have_age_and_attachment_geometry() {
        let propagule = definitions("minecraft:mangrove_propagule").unwrap();
        assert_eq!(
            propagule.blockstate["variants"].as_object().unwrap().len(),
            10
        );
        let shelf = definitions("minecraft:shelf_mushroom").unwrap();
        assert_eq!(shelf.blockstate["variants"].as_object().unwrap().len(), 8);
        for value in shelf.models.values() {
            assert_eq!(value["elements"][0]["to"][2], 16);
            assert!(value["elements"][0]["to"][1].as_i64().unwrap() < 16);
        }
    }
}

#[cfg(test)]
mod soil_face_tests {
    use super::*;
    #[test]
    fn soil_caps_have_distinct_pack_top_sides_and_dirt_bottom() {
        for block in ["mycelium", "podzol"] {
            let geometry = definitions(&format!("minecraft:{block}"))
                .expect("soil cap must have explicit geometry");
            assert_eq!(geometry.models.len(), 1);
            let model = geometry.models.values().next().unwrap();
            let element = &model["elements"][0];
            assert_eq!(element["from"], json!([0, 0, 0]));
            assert_eq!(element["to"], json!([16, 16, 16]));
            assert_eq!(
                element["faces"]["up"]["texture"],
                format!("minecraft:block/{block}_top")
            );
            assert_eq!(element["faces"]["down"]["texture"], "minecraft:block/dirt");
            for face in ["north", "south", "east", "west"] {
                assert_eq!(
                    element["faces"][face]["texture"],
                    format!("minecraft:block/{block}_side")
                );
            }
        }
    }
}

#[cfg(test)]
mod remaining_material_tests {
    use super::*;

    fn elements<'a>(geometry: &'a StateGeometry, key: &str) -> &'a [Value] {
        let id = geometry.blockstate["variants"][key]["model"]
            .as_str()
            .unwrap();
        geometry.models[id]["elements"].as_array().unwrap()
    }

    fn contains(elements: &[Value], point: [i32; 3]) -> bool {
        elements.iter().any(|element| {
            (0..3).all(|axis| {
                element["from"][axis].as_i64().unwrap() <= i64::from(point[axis])
                    && i64::from(point[axis]) < element["to"][axis].as_i64().unwrap()
            })
        })
    }

    #[test]
    fn inventory_materials_have_original_models_and_state_orientations() {
        for id in [
            "campfire",
            "chest",
            "cauldron",
            "bell",
            "composter",
            "oak_fence_gate",
            "jungle_fence_gate",
            "acacia_fence_gate",
            "spruce_fence_gate",
        ] {
            let full = format!("minecraft:{id}");
            assert!(is_remaining_generated_material(&full));
            assert!(definitions(&full).is_some());
        }
        assert!(!is_remaining_generated_material("minecraft:oak_fence"));
        assert!(!is_remaining_generated_material("custom:chest"));
        let camp = definitions("minecraft:campfire").unwrap();
        assert_eq!(camp.blockstate["variants"].as_object().unwrap().len(), 8);
        for (facing, angle) in FACING {
            for lit in [false, true] {
                let variant = &camp.blockstate["variants"][format!("facing={facing},lit={lit}")];
                assert_eq!(variant["y"], angle);
                let elements = elements(&camp, &format!("facing={facing},lit={lit}"));
                assert_eq!(elements.len(), if lit { 8 } else { 4 });
                assert_eq!(
                    elements[0]["faces"]["north"]["texture"],
                    format!(
                        "minecraft:block/campfire_log{}",
                        if lit { "_lit" } else { "" }
                    )
                );
            }
        }
        let chest = definitions("minecraft:chest").unwrap();
        assert_eq!(chest.blockstate["variants"].as_object().unwrap().len(), 4);
        for (facing, angle) in FACING {
            let key = format!("facing={facing},type=single");
            assert_eq!(chest.blockstate["variants"][&key]["y"], angle);
            assert_eq!(elements(&chest, &key).len(), 3);
        }
        assert_eq!(
            elements(&chest, "facing=north,type=single")[0]["faces"]["up"]["uv"],
            json!([3.5, 4.75, 7.0, 8.25])
        );
        assert_eq!(
            elements(&chest, "facing=north,type=single")[1]["faces"]["north"]["uv"],
            json!([3.5, 3.5, 7.0, 4.75])
        );
        assert!(chest
            .models
            .values()
            .flat_map(|model| model["elements"].as_array().unwrap())
            .flat_map(|element| element["faces"].as_object().unwrap().values())
            .all(|face| face["texture"] == "minecraft:entity/chest/normal"));
    }

    #[test]
    fn campfire_unlit_has_four_low_horizontal_solid_logs_instead_of_billboards() {
        let camp = definitions("minecraft:campfire").unwrap();
        let logs = elements(&camp, "facing=north,lit=false");
        assert_eq!(logs.len(), 4);
        let expected = [
            ([0, 0, 1], [16, 4, 5]),
            ([0, 0, 11], [16, 4, 15]),
            ([1, 4, 0], [5, 8, 16]),
            ([11, 4, 0], [15, 8, 16]),
        ];
        for (element, (from, to)) in logs.iter().zip(expected) {
            assert_eq!(element["from"], json!(from));
            assert_eq!(element["to"], json!(to));
            assert_eq!(element["faces"].as_object().unwrap().len(), 6);
            assert_eq!(to[1] - from[1], 4);
            assert!(to[1] <= 8);
            assert!(to[0] - from[0] == 16 || to[2] - from[2] == 16);
        }
    }

    #[test]
    fn campfire_bark_and_end_grain_use_opaque_source_art_islands() {
        let camp = definitions("minecraft:campfire").unwrap();
        for lit in [false, true] {
            let logs = elements(&camp, &format!("facing=north,lit={lit}"));
            for (index, log) in logs.iter().take(4).enumerate() {
                let texture = format!(
                    "minecraft:block/campfire_log{}",
                    if lit { "_lit" } else { "" }
                );
                let along_x = index < 2;
                for (face_name, face) in log["faces"].as_object().unwrap() {
                    let is_end = if along_x {
                        matches!(face_name.as_str(), "west" | "east")
                    } else {
                        matches!(face_name.as_str(), "north" | "south")
                    };
                    assert_eq!(face["texture"], texture);
                    assert_eq!(
                        face["uv"],
                        if is_end {
                            json!([0, 4, 4, 8])
                        } else {
                            json!([0, 0, 16, 4])
                        }
                    );
                    if !along_x && matches!(face_name.as_str(), "up" | "down") {
                        assert_eq!(face["rotation"], 90);
                    } else {
                        assert!(face.get("rotation").is_none());
                    }
                }
            }
        }
    }

    #[test]
    fn campfire_lit_retains_fire_cutouts_with_the_same_solid_log_stack() {
        let camp = definitions("minecraft:campfire").unwrap();
        let unlit = elements(&camp, "facing=north,lit=false");
        let lit = elements(&camp, "facing=north,lit=true");
        assert_eq!(lit.len(), 8);
        for (dark, burning) in unlit.iter().zip(lit.iter().take(4)) {
            assert_eq!(dark["from"], burning["from"]);
            assert_eq!(dark["to"], burning["to"]);
            assert_eq!(dark["faces"].as_object().unwrap().len(), 6);
            assert_eq!(burning["faces"].as_object().unwrap().len(), 6);
        }
        for flame in lit.iter().skip(4) {
            assert_eq!(flame["from"][1], 0);
            assert_eq!(flame["to"][1], 16);
            assert_eq!(flame["from"][2], flame["to"][2]);
            assert_eq!(flame["faces"].as_object().unwrap().len(), 2);
            for face in flame["faces"].as_object().unwrap().values() {
                assert_eq!(face["texture"], "minecraft:block/campfire_fire");
            }
        }
    }

    #[test]
    fn vessels_are_hollow_and_open_gate_leaves_clear_the_center() {
        let cauldron = definitions("minecraft:cauldron").unwrap();
        let empty = elements(&cauldron, "");
        assert_eq!(empty.len(), 5);
        assert!(!contains(empty, [8, 12, 8]));
        assert!(contains(empty, [8, 2, 8]));
        assert_eq!(
            empty[0]["faces"]["up"]["texture"],
            "minecraft:block/cauldron_inner"
        );
        let composter = definitions("minecraft:composter").unwrap();
        assert_eq!(
            composter.blockstate["variants"].as_object().unwrap().len(),
            9
        );
        let level0 = elements(&composter, "level=0");
        assert_eq!(level0.len(), 5);
        assert!(!contains(level0, [8, 12, 8]));
        assert_eq!(
            level0[1]["faces"]["up"]["texture"],
            "minecraft:block/composter_top"
        );
        assert_eq!(
            elements(&composter, "level=8")[5]["faces"]["up"]["texture"],
            "minecraft:block/composter_ready"
        );
        for species in ["oak", "jungle", "acacia", "spruce"] {
            let gate = definitions(&format!("minecraft:{species}_fence_gate")).unwrap();
            assert_eq!(gate.blockstate["variants"].as_object().unwrap().len(), 16);
            let open = elements(&gate, "facing=north,in_wall=false,open=true");
            let closed = elements(&gate, "facing=north,in_wall=false,open=false");
            assert!(!contains(open, [8, 5, 8]));
            assert!(contains(closed, [8, 5, 8]));
            assert_eq!(
                open[0]["faces"]["north"]["texture"],
                format!("minecraft:block/{species}_planks")
            );
        }
    }

    #[test]
    fn ceiling_bell_selects_the_actual_profile_specific_image_layout() {
        let atlas = definitions("minecraft:bell").unwrap();
        let faces = definitions_for_profile("minecraft:bell", true).unwrap();
        assert_eq!(atlas.blockstate["variants"].as_object().unwrap().len(), 4);
        for (facing, angle) in FACING {
            let key = format!("attachment=ceiling,facing={facing}");
            assert_eq!(atlas.blockstate["variants"][&key]["y"], angle);
            assert_eq!(faces.blockstate["variants"][&key]["y"], angle);
        }
        let body = &elements(&atlas, "attachment=ceiling,facing=north")[0];
        assert_eq!(body["faces"]["up"]["uv"], json!([3.0, 0.0, 6.0, 3.0]));
        assert_eq!(
            body["faces"]["north"]["texture"],
            "minecraft:entity/bell/bell_body"
        );
        let body = &elements(&faces, "attachment=ceiling,facing=north")[0];
        assert_eq!(
            body["faces"]["north"]["texture"],
            "minecraft:block/bell_side"
        );
        assert_eq!(body["faces"]["up"]["texture"], "minecraft:block/bell_top");
        assert_eq!(
            body["faces"]["down"]["texture"],
            "minecraft:block/bell_bottom"
        );
        assert_eq!(body["faces"]["north"]["uv"], json!([0, 0, 8, 8]));
    }
}
