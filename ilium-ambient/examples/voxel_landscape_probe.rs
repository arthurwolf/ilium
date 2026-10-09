//! Capture the production prepared-world/color pipeline. All stdout is JSONL.
//! --output DIR --width CELLS --height CELLS --seed U32 --x BLOCK --y BLOCK
//! --zoom PERCENT --detail 0..3 --mono --dither ordered|stippled --density 0..1
//! --palette 0..3 --hue 0..360 --saturation 0..100 --lightness 5..100 --camera-height BLOCK
//! --inspect-ecology adds center biome and final visible material/biome counts.
//! --texture-source selected-pack|java-default chooses production texture assets.
//! --pack-profile 0..7 --pack-path ABSOLUTE_PATH [--pack-mount archive|directory]
//! Matrix mode accepts --profile 0..7 or --texture-source java-default.
//! Profile order is the current FULL_PACKS order, not legacy persisted indices.
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
fn selected_pack(
    arguments: &mut BTreeMap<String, String>,
) -> Result<Option<(usize, PathBuf, usize)>, String> {
    let profile = arguments.remove("--pack-profile");
    let path = arguments.remove("--pack-path");
    let mount = arguments.remove("--pack-mount");
    match (profile, path) {
        (None, None) if mount.is_none() => Ok(None),
        (Some(profile), Some(path)) => {
            let profile = profile
                .parse::<usize>()
                .map_err(|_| format!("Invalid --pack-profile: {profile}"))?;
            if profile >= ilium_ambient::voxel_landscape::pack_profiles::FULL_PACKS.len() {
                return Err(format!("Unknown --pack-profile: {profile}"));
            }
            let path = PathBuf::from(path);
            if !path.is_absolute() {
                return Err("--pack-path must be absolute".into());
            }
            let mount = match mount.as_deref().unwrap_or("archive") {
                "archive" => 0,
                "directory" => 1,
                other => return Err(format!("Unknown --pack-mount: {other}")),
            };
            Ok(Some((profile, path, mount)))
        }
        _ => Err("--pack-profile and --pack-path must be supplied together".into()),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GeneratedTextureSource {
    SelectedPack,
    JavaDefault,
}

impl GeneratedTextureSource {
    fn parse(value: Option<&str>) -> Result<Self, String> {
        match value.unwrap_or("selected-pack") {
            "selected-pack" => Ok(Self::SelectedPack),
            "java-default" => Ok(Self::JavaDefault),
            other => Err(format!("Unknown --texture-source: {other}")),
        }
    }

    fn setting_index(self) -> usize {
        match self {
            Self::SelectedPack => 0,
            Self::JavaDefault => {
                ilium_ambient::voxel_landscape::GENERATED_TEXTURE_SOURCE_JAVA_DEFAULT
            }
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::SelectedPack => "selected-pack",
            Self::JavaDefault => "java-default-1.19.3",
        }
    }
}

fn qualified_texture_frame(
    frame: &ilium_ambient::voxel_landscape::surface_binding::StreamedViewport,
) -> Result<(usize, String), String> {
    let (satisfied, required) = frame.material_coverage;
    if required == 0 || satisfied != required {
        return Err(format!(
            "Texture frame has incomplete required material coverage: {satisfied}/{required}"
        ));
    }
    let covered_pixels = frame.covered.iter().filter(|covered| **covered).count();
    if covered_pixels == 0 {
        return Err("Texture frame contains no visible covered pixels".into());
    }
    let source_sha256 = frame
        .source_sha256
        .ok_or_else(|| "Texture frame has no source archive digest".to_owned())?
        .bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Ok((covered_pixels, source_sha256))
}

fn covered_texture_pixels(colors: &[[u8; 3]], covered: &[bool]) -> Result<Vec<u8>, String> {
    if colors.len() != covered.len() {
        return Err(format!(
            "Texture color and coverage lengths differ: {} colors, {} coverage flags",
            colors.len(),
            covered.len()
        ));
    }
    let mut pixels = vec![0; colors.len() * 3];
    for (index, (color, is_covered)) in colors.iter().zip(covered).enumerate() {
        if *is_covered {
            pixels[index * 3..index * 3 + 3].copy_from_slice(color);
        }
    }
    Ok(pixels)
}

fn parse_capture_texture_source(
    arguments: &mut BTreeMap<String, String>,
) -> Result<(GeneratedTextureSource, Option<(usize, PathBuf, usize)>), String> {
    let requested_source = arguments.remove("--texture-source");
    let source = GeneratedTextureSource::parse(requested_source.as_deref())?;
    let selected_pack = selected_pack(arguments)?;
    if requested_source.is_some()
        && source == GeneratedTextureSource::SelectedPack
        && selected_pack.is_none()
    {
        return Err(
            "--texture-source selected-pack requires --pack-profile and --pack-path".into(),
        );
    }
    if source == GeneratedTextureSource::JavaDefault && selected_pack.is_some() {
        return Err(
            "--texture-source java-default cannot be combined with --pack-profile/--pack-path"
                .into(),
        );
    }
    Ok((source, selected_pack))
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

struct MatrixArguments {
    cache_root: PathBuf,
    output: PathBuf,
    profile: Option<usize>,
    texture_source: GeneratedTextureSource,
    biome: String,
    x: i32,
    y: i32,
    seed: u32,
    zoom: u32,
    size: [usize; 2],
    vegetation: i32,
    atmosphere: usize,
    times_ms: Vec<u64>,
}

fn parse_matrix_arguments(
    values: impl IntoIterator<Item = String>,
) -> Result<MatrixArguments, String> {
    let mut arguments = BTreeMap::new();
    let mut values = values.into_iter();
    while let Some(flag) = values.next() {
        if flag == "--matrix" {
            continue;
        }
        if !flag.starts_with("--") {
            return Err(format!("Expected a flag, got {flag}"));
        }
        let value = values
            .next()
            .ok_or_else(|| format!("Missing value for {flag}"))?;
        if arguments.insert(flag.clone(), value).is_some() {
            return Err(format!("Duplicate {flag}"));
        }
    }
    let texture_source =
        GeneratedTextureSource::parse(arguments.remove("--texture-source").as_deref())?;
    let profile = match texture_source {
        GeneratedTextureSource::SelectedPack => Some(
            arguments
                .remove("--profile")
                .ok_or_else(|| "--profile is required for selected-pack captures".to_owned())?
                .parse::<usize>()
                .map_err(|_| "Invalid --profile".to_string())?,
        ),
        GeneratedTextureSource::JavaDefault => {
            if arguments.remove("--profile").is_some() {
                return Err(
                    "--profile cannot be combined with --texture-source java-default".into(),
                );
            }
            None
        }
    };
    if let Some(profile) = profile {
        if profile >= ilium_ambient::voxel_landscape::pack_profiles::FULL_PACKS.len() {
            return Err(format!("Unknown --profile: {profile}"));
        }
    }
    let mut parse = |name: &str| {
        arguments
            .remove(name)
            .ok_or_else(|| format!("{name} is required"))
    };
    let cache_root = PathBuf::from(parse("--cache-root")?);
    let output = PathBuf::from(parse("--output")?);
    if !cache_root.is_absolute() || !output.is_absolute() {
        return Err("--cache-root and --output must be absolute paths".into());
    }
    let biome = parse("--biome")?;
    if biome.is_empty() || biome.chars().any(char::is_control) {
        return Err("--biome must be a non-empty identifier".into());
    }
    let x = parse("--x")?
        .parse::<i32>()
        .map_err(|_| "Invalid --x".to_string())?;
    let y = parse("--y")?
        .parse::<i32>()
        .map_err(|_| "Invalid --y".to_string())?;
    let zoom = parse("--zoom")?
        .parse::<u32>()
        .map_err(|_| "Invalid --zoom".to_string())?;
    if !(1..=400).contains(&zoom) {
        return Err("--zoom must be in 1..=400".into());
    }
    let width = parse("--width")?
        .parse::<usize>()
        .map_err(|_| "Invalid --width".to_string())?;
    let height = parse("--height")?
        .parse::<usize>()
        .map_err(|_| "Invalid --height".to_string())?;
    if width == 0
        || width > 1000
        || width % 2 != 0
        || height == 0
        || height > 800
        || height % 4 != 0
    {
        return Err(
            "Pixel size must be even-width, 4-row aligned, and within 1..1000 × 1..800".into(),
        );
    }
    let seed = parse("--seed")?
        .parse::<u32>()
        .map_err(|_| "Invalid --seed".to_string())?;
    let vegetation = parse("--vegetation")?
        .parse::<i32>()
        .map_err(|_| "Invalid --vegetation".to_string())?;
    if !(0..=200).contains(&vegetation) {
        return Err("--vegetation must be in 0..=200".into());
    }
    let atmosphere = parse("--atmosphere")?
        .parse::<usize>()
        .map_err(|_| "Invalid --atmosphere".to_string())?;
    if atmosphere > 2 {
        return Err("--atmosphere must be in 0..=2".into());
    }
    let times = parse("--times-ms")?
        .split(',')
        .map(|value| {
            value
                .parse::<u64>()
                .map_err(|_| "Invalid --times-ms".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    if times.is_empty() || times.len() > 8 || times.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err("--times-ms requires 1..=8 strictly increasing nonnegative times".into());
    }
    if !arguments.is_empty() {
        return Err(format!("Unknown flags: {:?}", arguments.keys()));
    }
    Ok(MatrixArguments {
        cache_root,
        output,
        profile,
        texture_source,
        biome,
        x,
        y,
        seed,
        zoom,
        size: [width, height],
        vegetation,
        atmosphere,
        times_ms: times,
    })
}

fn generated_center_biome(settings: &VoxelLandscapeSettings, position: [i32; 2]) -> &'static str {
    let seed = u64::from(settings.seed);
    let sample = ilium_ambient::voxel_landscape::terrain_fields::TerrainFields::new(seed).sample(
        position[0],
        position[1],
        settings.rivers,
    );
    ilium_ambient::voxel_landscape::surface_biome_selector::select(seed, position, sample).id()
}

fn require_requested_biome(requested: &str, actual: &str) -> Result<(), String> {
    if requested == actual {
        Ok(())
    } else {
        Err(format!(
            "requested biome {requested} does not match generated center biome {actual}"
        ))
    }
}

fn capture_matrix(values: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    let arguments = parse_matrix_arguments(values)?;
    let stopped = std::sync::atomic::AtomicBool::new(false);
    let cancel = ilium_ambient::voxel_landscape::assets::Cancel::new(&stopped);
    let mut settings = VoxelLandscapeSettings {
        pack_profile: arguments.profile.unwrap_or_default(),
        generated_texture_source: arguments.texture_source.setting_index(),
        seed: arguments.seed,
        zoom_percent: i32::try_from(arguments.zoom)?,
        vegetation_percent: arguments.vegetation,
        atmosphere: arguments.atmosphere,
        ..VoxelLandscapeSettings::default()
    }
    .normalized();
    if arguments.texture_source == GeneratedTextureSource::SelectedPack {
        settings = ilium_ambient::voxel_landscape::pack_registry::resolve_registered(
            &settings,
            &arguments.cache_root,
            cancel,
        )?;
    }
    let generated_center_biome = generated_center_biome(&settings, [arguments.x, arguments.y]);
    require_requested_biome(&arguments.biome, generated_center_biome)?;
    let camera = [f64::from(arguments.x), f64::from(arguments.y), 26.0];
    let scale = VoxelLandscapeScene::scale(&settings);
    let region = VoxelLandscapeScene::surface_region(camera, scale, arguments.size)?;
    let mut session =
        ilium_ambient::voxel_landscape::surface_binding::GeneratedViewportSession::open(
            region,
            scale,
            arguments.size,
            &settings,
            None,
            ilium_ambient::voxel_landscape::surface_binding::scene_budget()?,
            cancel,
        )?;
    let mut frames = Vec::with_capacity(arguments.times_ms.len());
    for time_ms in arguments.times_ms {
        let frame = session.render(camera, Duration::from_millis(time_ms), cancel)?;
        let (covered_pixels, source_sha256) = qualified_texture_frame(&frame)?;
        let path = arguments.output.join(format!("frame-{time_ms:010}.png"));
        let pixels = covered_texture_pixels(&frame.colors, &frame.covered)?;
        png(
            &path,
            &pixels,
            arguments.size[0] as u32,
            arguments.size[1] as u32,
        )?;
        frames.push(serde_json::json!({
            "time_ms": time_ms,
            "path": path,
            "source_sha256": source_sha256,
            "covered_pixels": covered_pixels,
            "total_pixels": frame.covered.len(),
            "material_coverage": [frame.material_coverage.0, frame.material_coverage.1],
            "compatibility_aliases": frame.compatibility_aliases,
            "material_fallbacks": frame.material_fallbacks,
            "tile_count": frame.tile_count,
            "status": frame.status,
        }));
    }
    emit(serde_json::json!({
        "type": "result",
        "profile": arguments.profile,
        "texture_source": arguments.texture_source.label(),
        "biome": arguments.biome,
        "generated_center_biome": generated_center_biome,
        "seed": arguments.seed,
        "center": [arguments.x, arguments.y],
        "zoom": arguments.zoom,
        "size": arguments.size,
        "frames": frames,
    }));
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
    let (texture_source, selected_pack) = parse_capture_texture_source(&mut arguments)?;
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
    settings.generated_texture_source = texture_source.setting_index();
    if let Some((profile, path, mount)) = &selected_pack {
        settings.pack_profile = *profile;
        settings.pack_path = path.to_string_lossy().into_owned();
        settings.pack_mount = *mount;
    }
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
    if texture_source == GeneratedTextureSource::JavaDefault || selected_pack.is_some() {
        let selected_profile = selected_pack
            .as_ref()
            .map(|(profile, path, _)| (*profile, path.clone()));
        let scale = VoxelLandscapeScene::scale(&settings);
        let surface_region = VoxelLandscapeScene::surface_region(camera, scale, size)?;
        let stopped = std::sync::atomic::AtomicBool::new(false);
        let cancel = ilium_ambient::voxel_landscape::assets::Cancel::new(&stopped);
        let started = Instant::now();
        let mut session =
            ilium_ambient::voxel_landscape::surface_binding::GeneratedViewportSession::open(
                surface_region,
                scale,
                size,
                &settings,
                None,
                ilium_ambient::voxel_landscape::surface_binding::scene_budget()?,
                cancel,
            )?;
        let loaded_ms = started.elapsed().as_secs_f64() * 1000.0;
        let frame = session.render(camera, Duration::ZERO, cancel)?;
        let (covered_pixels, source_sha256) = qualified_texture_frame(&frame)?;
        let rendered_ms = started.elapsed().as_secs_f64() * 1000.0 - loaded_ms;
        let pack_pixels = covered_texture_pixels(&frame.colors, &frame.covered)?;
        let source_image_name = if texture_source == GeneratedTextureSource::JavaDefault {
            "java-default.png"
        } else {
            "selected-pack.png"
        };
        let texture_png = output.join(source_image_name);
        png(&texture_png, &pack_pixels, size[0] as u32, size[1] as u32)?;
        let (profile_id, profile_name, source_path) =
            selected_profile.map_or((None, None, None), |(profile, path)| {
                (
                    Some(ilium_ambient::voxel_landscape::pack_profiles::FULL_PACKS[profile].id),
                    Some(ilium_ambient::voxel_landscape::pack_profiles::FULL_PACKS[profile].name),
                    Some(path),
                )
            });
        report["texture_render"] = serde_json::json!({
            "source": texture_source.label(),
            "profile_id": profile_id,
            "profile_name": profile_name,
            "source_path": source_path,
            "source_sha256": source_sha256,
            "material_coverage": {"satisfied": frame.material_coverage.0, "required": frame.material_coverage.1},
            "covered_pixels": covered_pixels,
            "total_pixels": frame.covered.len(),
            "image_path": texture_png,
            "load_ms": loaded_ms,
            "render_ms": rendered_ms,
            "compatibility_aliases": frame.compatibility_aliases,
            "material_fallbacks": frame.material_fallbacks,
            "status": frame.status,
            "scope": "Generated viewport rendered by the production texture asset, model, and material pipeline; not a saved-world or native-client acceptance result"
        });
        if let Some(selected_pack) = report.get("texture_render").cloned() {
            if texture_source == GeneratedTextureSource::SelectedPack {
                report["selected_pack"] = selected_pack;
            }
        }
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
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    let result = if arguments.iter().any(|argument| argument == "--matrix") {
        capture_matrix(arguments)
    } else {
        capture()
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            emit(serde_json::json!({"type":"error","message":error.to_string()}));
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        covered_texture_pixels, parse_matrix_arguments, selected_pack, VoxelLandscapeSettings,
    };
    use std::collections::BTreeMap;

    fn matrix_arguments() -> Vec<String> {
        [
            "--matrix",
            "--cache-root",
            "/cache",
            "--output",
            "/output",
            "--profile",
            "0",
            "--biome",
            "minecraft:plains",
            "--x",
            "-100",
            "--y",
            "250",
            "--zoom",
            "100",
            "--width",
            "480",
            "--height",
            "320",
            "--seed",
            "42",
            "--vegetation",
            "100",
            "--atmosphere",
            "0",
            "--times-ms",
            "0,240",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect()
    }

    #[test]
    fn matrix_arguments_match_runner_pixels_and_requested_frames() {
        let parsed = parse_matrix_arguments(matrix_arguments()).unwrap();
        assert_eq!(parsed.profile, Some(0));
        assert_eq!(parsed.biome, "minecraft:plains");
        assert_eq!(parsed.size, [480, 320]);
        assert_eq!(parsed.times_ms, [0, 240]);

        let mut last_pack = matrix_arguments();
        let profile = last_pack
            .iter()
            .position(|value| value == "--profile")
            .unwrap()
            + 1;
        last_pack[profile] = "7".into();
        assert_eq!(parse_matrix_arguments(last_pack).unwrap().profile, Some(7));

        for unsupported_profile in ["8", "java-default"] {
            let mut unsupported = matrix_arguments();
            let profile = unsupported
                .iter()
                .position(|value| value == "--profile")
                .unwrap()
                + 1;
            unsupported[profile] = unsupported_profile.into();
            assert!(parse_matrix_arguments(unsupported).is_err());
        }
    }

    #[test]
    fn matrix_accepts_java_default_without_a_profile_and_rejects_mixed_sources() {
        let mut java_default = matrix_arguments();
        let profile = java_default
            .iter()
            .position(|value| value == "--profile")
            .unwrap();
        java_default.drain(profile..=profile + 1);
        java_default.extend(["--texture-source".into(), "java-default".into()]);
        let parsed = parse_matrix_arguments(java_default.clone()).unwrap();
        assert_eq!(parsed.profile, None);
        assert_eq!(
            parsed.texture_source,
            super::GeneratedTextureSource::JavaDefault
        );

        java_default.extend(["--profile".into(), "2".into()]);
        assert!(parse_matrix_arguments(java_default).is_err());
    }

    #[test]
    fn capture_texture_source_requires_a_single_explicit_source() {
        let mut default = BTreeMap::new();
        assert_eq!(
            super::parse_capture_texture_source(&mut default).unwrap().0,
            super::GeneratedTextureSource::SelectedPack
        );

        let mut selected_without_pack =
            BTreeMap::from([("--texture-source".into(), "selected-pack".into())]);
        assert!(super::parse_capture_texture_source(&mut selected_without_pack).is_err());

        let mut java_default = BTreeMap::from([("--texture-source".into(), "java-default".into())]);
        assert_eq!(
            super::parse_capture_texture_source(&mut java_default)
                .unwrap()
                .0,
            super::GeneratedTextureSource::JavaDefault
        );

        let mut conflicting = BTreeMap::from([
            ("--texture-source".into(), "java-default".into()),
            ("--pack-profile".into(), "2".into()),
            ("--pack-path".into(), "/packs/faithful.zip".into()),
        ]);
        assert!(super::parse_capture_texture_source(&mut conflicting).is_err());
    }

    #[test]
    fn texture_png_pixels_hide_uncovered_colors_and_reject_misaligned_masks() {
        let colors = [[21, 170, 47], [32, 90, 210]];
        assert_eq!(
            covered_texture_pixels(&colors, &[true, false]).unwrap(),
            [21, 170, 47, 0, 0, 0]
        );
        assert!(covered_texture_pixels(&colors, &[true]).is_err());
    }

    #[test]
    fn matrix_center_biome_uses_the_generated_terrain_classifier() {
        let settings = VoxelLandscapeSettings {
            seed: 71839,
            ..VoxelLandscapeSettings::default()
        }
        .normalized();
        assert_eq!(
            generated_center_biome(&settings, [-15_104, -16_384]),
            "minecraft:forest"
        );
    }

    #[test]
    fn matrix_rejects_a_requested_biome_that_does_not_match_the_generated_center() {
        assert_eq!(
            super::require_requested_biome("minecraft:forest", "minecraft:forest"),
            Ok(())
        );
        let error =
            super::require_requested_biome("minecraft:river", "minecraft:forest").unwrap_err();
        assert_eq!(
            error,
            "requested biome minecraft:river does not match generated center biome minecraft:forest"
        );
    }

    #[test]
    fn matrix_arguments_reject_non_pixel_aligned_sizes_and_unknown_flags() {
        let mut bad_size = matrix_arguments();
        let width = bad_size
            .iter()
            .position(|value| value == "--width")
            .unwrap()
            + 1;
        bad_size[width] = "479".into();
        assert!(parse_matrix_arguments(bad_size).is_err());

        let mut unknown = matrix_arguments();
        unknown.extend(["--unexpected".into(), "yes".into()]);
        assert!(parse_matrix_arguments(unknown).is_err());
    }

    #[test]
    fn selected_pack_accepts_exactly_the_current_eight_indices() {
        for index in 0..8 {
            let mut arguments = BTreeMap::from([
                ("--pack-profile".into(), index.to_string()),
                ("--pack-path".into(), "/packs/current.zip".into()),
            ]);
            assert_eq!(selected_pack(&mut arguments).unwrap().unwrap().0, index);
        }
        for index in [8, 9, 10, usize::MAX] {
            let mut arguments = BTreeMap::from([
                ("--pack-profile".into(), index.to_string()),
                ("--pack-path".into(), "/packs/retired.zip".into()),
            ]);
            assert!(selected_pack(&mut arguments).is_err());
        }
    }

    #[test]
    fn selected_pack_requires_profile_and_absolute_path() {
        let mut profile_only = BTreeMap::from([("--pack-profile".into(), "3".into())]);
        assert!(selected_pack(&mut profile_only).is_err());

        let mut relative_path = BTreeMap::from([
            ("--pack-profile".into(), "3".into()),
            ("--pack-path".into(), "packs/faithful.zip".into()),
        ]);
        assert!(selected_pack(&mut relative_path).is_err());
    }

    #[test]
    fn selected_pack_accepts_archive_and_directory_mounts() {
        let mut archive = BTreeMap::from([
            ("--pack-profile".into(), "3".into()),
            ("--pack-path".into(), "/packs/faithful.zip".into()),
        ]);
        assert_eq!(selected_pack(&mut archive).unwrap().unwrap().2, 0);

        let mut directory = BTreeMap::from([
            ("--pack-profile".into(), "3".into()),
            ("--pack-path".into(), "/packs/faithful".into()),
            ("--pack-mount".into(), "directory".into()),
        ]);
        assert_eq!(selected_pack(&mut directory).unwrap().unwrap().2, 1);
    }
}
