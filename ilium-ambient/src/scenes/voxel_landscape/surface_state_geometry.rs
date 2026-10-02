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

pub fn definitions(id: &str) -> Option<StateGeometry> {
    let block = id.strip_prefix("minecraft:")?;
    let mut result = StateGeometry {
        models: BTreeMap::new(),
        blockstate: Value::Null,
    };
    let mut variants = Map::new();
    if block == "cactus" {
        let mut element = cuboid([1, 0, 1], [15, 16, 15], "cactus_side");
        for (face, texture) in [("up", "cactus_top"), ("down", "cactus_bottom")] {
            element["faces"][face]["texture"] = json!(format!("minecraft:block/{texture}"));
        }
        let id = model(&mut result, block, "inset", vec![element]);
        variants.insert(String::new(), json!({"model":id}));
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
    } else if matches!(block, "melon" | "pale_moss_block") {
        let texture = if block == "melon" {
            "melon_side"
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
