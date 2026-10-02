//! Capture the production prepared-world/color pipeline. All stdout is JSONL.
//! --output DIR --width CELLS --height CELLS --seed U32 --x BLOCK --y BLOCK
//! --zoom PERCENT --detail 0..3 --mono --dither ordered|stippled --density 0..1
//! --palette 0..3 --hue 0..360 --saturation 0..100 --lightness 5..100 --camera-height BLOCK
//! --inspect-ecology adds center biome and final visible material/biome counts.
use ilium_ambient::{
    control::SceneSettings,
    raster::{self, DitherMode, Raster},
    scene::Frame,
    voxel_landscape::{
        catalog::FEATURE_RECIPES,
        chunks::ColumnCache,
        ecology,
        engine::{self, VoxelLandscapeScene},
        generation, terrain, VoxelLandscapeSettings,
    },
};
use image::ImageEncoder;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime},
};

fn emit(value: serde_json::Value) {
    println!("{value}");
}
fn number<T: std::str::FromStr>(
    arguments: &mut BTreeMap<String, String>,
    name: &str,
    default: T,
) -> Result<T, String> {
    arguments.remove(name).map_or(Ok(default), |text| {
        text.parse().map_err(|_| format!("Invalid {name}: {text}"))
    })
}
fn png(
    path: &Path,
    pixels: &[u8],
    width: u32,
    height: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    image::codecs::png::PngEncoder::new(file).write_image(
        pixels,
        width,
        height,
        image::ExtendedColorType::Rgb8,
    )?;
    emit(serde_json::json!({"type":"artifact","path":path,"width":width,"height":height}));
    Ok(())
}
fn capture() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = BTreeMap::new();
    let mut flags = std::env::args().skip(1);
    while let Some(flag) = flags.next() {
        if flag == "--mono" || flag == "--inspect-ecology" {
            arguments.insert(flag, "true".into());
            continue;
        }
        if !flag.starts_with("--") {
            return Err(format!("Expected a flag, got {flag}").into());
        }
        let value = flags
            .next()
            .ok_or_else(|| format!("Missing value for {flag}"))?;
        if arguments.insert(flag.clone(), value).is_some() {
            return Err(format!("Duplicate {flag}").into());
        }
    }
    let output = PathBuf::from(
        arguments
            .remove("--output")
            .ok_or("--output DIR is required")?,
    );
    let width = number(&mut arguments, "--width", 160u16)?;
    let height = number(&mut arguments, "--height", 50u16)?;
    if !(1..=500).contains(&width) || !(1..=200).contains(&height) {
        return Err("Capture size outside 1..500 × 1..200 cells".into());
    }
    let mut settings = VoxelLandscapeSettings::default();
    settings.seed = number(&mut arguments, "--seed", settings.seed)?;
    settings.zoom_percent = number(&mut arguments, "--zoom", settings.zoom_percent)?;
    settings.detail = number(&mut arguments, "--detail", settings.detail)?;
    settings.palette = number(&mut arguments, "--palette", settings.palette)?;
    settings.hue_degrees = number(&mut arguments, "--hue", settings.hue_degrees)?;
    settings.saturation_percent =
        number(&mut arguments, "--saturation", settings.saturation_percent)?;
    settings.lightness_percent = number(&mut arguments, "--lightness", settings.lightness_percent)?;
    let camera_height = number(&mut arguments, "--camera-height", 26.0f64)?;
    if !camera_height.is_finite() || !(0.0..=120.0).contains(&camera_height) {
        return Err("Camera height must be finite in 0..120 blocks".into());
    }
    settings.color_mode = if arguments.remove("--mono").is_some() {
        0
    } else {
        1
    };
    let inspect_ecology = arguments.remove("--inspect-ecology").is_some();
    let x = number(&mut arguments, "--x", 64i32)?;
    let y = number(&mut arguments, "--y", 64i32)?;
    let density = number(&mut arguments, "--density", 0.6f32)?;
    if !density.is_finite() || !(0.0..=1.0).contains(&density) {
        return Err("Density must be finite in 0..1".into());
    }
    let mode = match arguments.remove("--dither").as_deref().unwrap_or("ordered") {
        "ordered" => DitherMode::Ordered,
        "stippled" => DitherMode::Stippled,
        _ => return Err("Unknown dither mode".into()),
    };
    if !arguments.is_empty() {
        return Err(format!("Unknown flags: {:?}", arguments.keys()).into());
    }
    settings = settings.normalized();
    let camera = [f64::from(x), f64::from(y), camera_height];
    let size = [usize::from(width) * 2, usize::from(height) * 4];
    let region = VoxelLandscapeScene::region(camera, VoxelLandscapeScene::scale(&settings), size);
    let started = Instant::now();
    let world = generation::prepare(region, &settings, &mut ColumnCache::new(256), || false)
        .ok_or("Invalid world region")?;
    let generation_ms = started.elapsed().as_secs_f64() * 1000.;
    let mut raster = Raster::default();
    raster.resize(size[0], size[1]);
    let mut colors = Vec::new();
    let started = Instant::now();
    let canvas = engine::render_prepared(
        &world,
        &mut Frame {
            raster: &mut raster,
            cell_colors: &mut colors,
            width,
            height,
            time: Duration::ZERO,
            wall: Duration::ZERO,
            now: SystemTime::UNIX_EPOCH,
        },
        &settings,
        camera,
    );
    let render_ms = started.elapsed().as_secs_f64() * 1000.;
    std::fs::create_dir_all(&output)?;
    let pixels: Vec<_> = canvas.colors.iter().flat_map(|color| *color).collect();
    png(
        &output.join("raw.png"),
        &pixels,
        size[0] as u32,
        size[1] as u32,
    )?;
    let mut dither = vec![0u8; size[0] * size[1] * 3];
    let mut depth_visible = BTreeSet::new();
    let mut final_lit = BTreeSet::new();
    let mut lit_dots = 0;
    for (index, owner) in canvas.block_owners.iter().enumerate() {
        if let Some(owner) = owner {
            depth_visible.insert(*owner);
        }
        let x = index % size[0];
        let y = index / size[0];
        if raster.dots[index] * density <= raster::threshold(x, y, mode) {
            continue;
        }
        lit_dots += 1;
        if let Some(owner) = owner {
            final_lit.insert(*owner);
        }
        dither[index * 3..index * 3 + 3]
            .copy_from_slice(&colors[(y / 4) * usize::from(width) + x / 2]);
    }
    png(
        &output.join("dither.png"),
        &dither,
        size[0] as u32,
        size[1] as u32,
    )?;
    let mut report = serde_json::json!({"type":"result","generator_version":terrain::GENERATOR_VERSION,"settings":settings,"camera":camera,"cells":[width,height],"candidate_blocks":world.blocks.len(),"depth_visible_blocks":depth_visible.len(),"final_lit_blocks":final_lit.len(),"lit_dots":lit_dots,"instances":world.instances.len(),"biomes":world.biomes.iter().map(|biome|biome.name()).collect::<Vec<_>>(),"cache_chunks":world.cached_chunks,"generation_ms":generation_ms,"render_ms":render_ms,"dither":mode,"density":density});
    if inspect_ecology {
        let kernel = terrain::Terrain::new(u64::from(settings.seed)).with_features(
            settings.rivers,
            settings.ravines,
            settings.caves,
        );
        let center_kernel = kernel.sample(x, y);
        let (center_occupied, center_ecology) =
            generation::sample_occupied_column(u64::from(settings.seed), x, y, center_kernel);
        let mut materials = BTreeMap::<String, usize>::new();
        let mut visible_biomes = BTreeMap::<String, usize>::new();
        let mut column_biomes = BTreeMap::new();
        for block in &world.blocks {
            if !final_lit.contains(&block.position) {
                continue;
            }
            *materials
                .entry(format!("{:?}", block.material))
                .or_default() += 1;
            let [bx, by, _] = block.position;
            let biome = *column_biomes.entry([bx, by]).or_insert_with(|| {
                ecology::sample(u64::from(settings.seed), bx, by, kernel.sample(bx, by)).biome
            });
            *visible_biomes.entry(biome.name().to_string()).or_default() += 1;
        }
        let mut features = BTreeMap::<String, usize>::new();
        for instance in &world.instances {
            if let Some(recipe) = FEATURE_RECIPES.get(usize::from(instance.id.0)) {
                *features.entry(recipe.name.to_string()).or_default() += 1;
            }
        }
        report["ecology_diagnostic"] = serde_json::json!({
            "center_biome":center_ecology.biome.name(),
            "center_kernel_height":center_kernel.height,
            "center_kernel_water_level":center_kernel.water_level,
            "center_occupied_height":center_occupied.height,
            "center_occupied_water_level":center_occupied.water_level,
            "final_visible_blocks_by_material":materials,
            "final_visible_blocks_by_source_biome":visible_biomes,
            "all_prepared_instances_by_recipe":features,
            "scope":"center_kernel_* is frozen terrain; center_occupied_* is production occupancy after raw biome classification; biome counts classify original block ground coordinates; overhanging features may originate elsewhere; instance counts include offscreen halo"
        });
    }
    let path = output.join("receipt.json");
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    std::fs::write(&path, serde_json::to_vec_pretty(&report)?)?;
    emit(report);
    emit(serde_json::json!({"type":"artifact","path":path}));
    Ok(())
}
fn main() -> std::process::ExitCode {
    match capture() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            emit(serde_json::json!({"type":"error","message":error.to_string()}));
            std::process::ExitCode::FAILURE
        }
    }
}
