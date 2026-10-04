//! Synthetic source/model/image fixtures only; these tests do not certify private pack art.
use super::super::assets::{
    bank::CoverageProblem,
    layers::MountLayout,
    models::oriented_quad,
    pixels::fixture_png,
    review::{fixture_origin, fixture_review, SourceEdition},
    source::{AssetSource, MemberInfo, SourceBytes},
};
use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

const STATE: &[u8] = br#"{"variants":{"axis=y":{"model":"minecraft:block/ilium_bone_block_column"},"axis=x":{"model":"minecraft:block/ilium_bone_block_column","x":90,"y":90},"axis=z":{"model":"minecraft:block/ilium_bone_block_column","x":90}}}"#;
const MODEL: &[u8] = br#"{"elements":[{"from":[0,8,0],"to":[8,8,16],"shade":false,"faces":{"up":{"texture":"minecraft:block/bone_block_top","uv":[0,0,16,16]}}}]}"#;
const STATE_PATH: &str = "assets/minecraft/blockstates/bone_block.json";
const MODEL_PATH: &str = "assets/minecraft/models/block/ilium_bone_block_column.json";
const SIDE_PATH: &str = "assets/minecraft/textures/block/bone_block_side.png";
const TOP_PATH: &str = "assets/minecraft/textures/block/bone_block_top.png";
const COARSE_PATH: &str = "assets/minecraft/textures/blocks/coarse_dirt.png";
const MATERIALS: [&str; 19] = [
    "andesite",
    "bone_block_side",
    "bone_block_top",
    "brown_terracotta",
    "calcite",
    "coarse_dirt",
    "dirt",
    "gravel",
    "light_gray_terracotta",
    "orange_terracotta",
    "podzol_side",
    "podzol_top",
    "red_sand",
    "red_terracotta",
    "sand",
    "stone",
    "terracotta",
    "white_terracotta",
    "yellow_terracotta",
];

struct GeologySource {
    members: BTreeMap<AssetPath, MemberInfo>,
    files: BTreeMap<AssetPath, SourceBytes>,
    _index_charge: Reservation,
}
impl GeologySource {
    fn new(
        entries: &[(&str, Option<&[u8]>)],
        budget: &ByteBudget,
        cancel: Cancel<'_>,
    ) -> Result<Self> {
        let mut result = Self {
            members: BTreeMap::new(),
            files: BTreeMap::new(),
            _index_charge: budget.reserve(4096 + entries.len() as u64 * 4096, cancel)?,
        };
        for &(path, bytes) in entries {
            let Some(bytes) = bytes else { continue };
            let path = AssetPath::parse(path)?;
            let retained =
                SourceBytes::from_slice(bytes, Limits::default().encoded_bytes, budget, cancel)?;
            let member = MemberInfo {
                path: path.clone(),
                bytes: bytes.len() as u64,
            };
            assert!(result.members.insert(path.clone(), member).is_none());
            result.files.insert(path, retained);
        }
        Ok(result)
    }
}
impl AssetSource for GeologySource {
    fn members(&self) -> &BTreeMap<AssetPath, MemberInfo> {
        &self.members
    }
    fn source_digest(&self) -> Option<Digest256> {
        None
    }
    fn read(
        &self,
        path: &AssetPath,
        cap: u64,
        budget: &ByteBudget,
        cancel: Cancel<'_>,
    ) -> Result<Option<SourceBytes>> {
        cancel.check()?;
        let Some(bytes) = self.files.get(path) else {
            return Ok(None);
        };
        if !bytes.uses_budget(budget) {
            return Err(AssetError::InvalidMetadata(
                "synthetic geology account differs".into(),
            ));
        }
        SourceBytes::from_slice(bytes.bytes(), cap, budget, cancel).map(Some)
    }
}
fn mounted(
    kind: OriginKind,
    metadata_case: u8,
    top: bool,
    png: &[Vec<u8>; 4],
    budget: &ByteBudget,
    cancel: Cancel<'_>,
) -> LayeredPack {
    let selected = kind == OriginKind::SelectedPack;
    let first = if selected { 0 } else { 2 };
    let model = if selected {
        MODEL
    } else {
        &b"{\"elements\":[]}"[..]
    };
    let source: Arc<dyn AssetSource> = Arc::new(
        GeologySource::new(
            &[
                (STATE_PATH, (metadata_case != 1).then_some(STATE)),
                (MODEL_PATH, (metadata_case != 2).then_some(model)),
                (SIDE_PATH, Some(png[first].as_slice())),
                (TOP_PATH, top.then_some(png[first + 1].as_slice())),
                (COARSE_PATH, Some(png[first].as_slice())),
            ],
            budget,
            cancel,
        )
        .unwrap(),
    );
    let mut review = fixture_review();
    review.pack = ResourceId::parse(if selected {
        "fixture:geology_selected"
    } else {
        "fixture:geology_fallback"
    })
    .unwrap();
    review.edition = SourceEdition::Java;
    LayeredPack::mount(
        review,
        source,
        None,
        MountLayout::Java,
        kind,
        Limits::default(),
        budget.clone(),
        cancel,
    )
    .unwrap()
}
fn compatibility(budget: &ByteBudget, cancel: Cancel<'_>) -> DefinitionSet {
    let mut definitions = DefinitionSet::new(
        fixture_origin(OriginKind::OriginalCompatibilityGeometry),
        Limits::default(),
        budget.clone(),
    )
    .unwrap();
    definitions.install_geometry_templates(cancel).unwrap();
    add_compatibility(
        &mut definitions,
        &ResourceId::parse("minecraft:bone_block").unwrap(),
        false,
        false,
        true,
        cancel,
    )
    .unwrap();
    definitions
}
fn bone_state(axis: &str) -> BlockState {
    BlockState::new(
        ResourceId::parse("minecraft:bone_block").unwrap(),
        [("axis".into(), axis.into())],
    )
    .unwrap()
}
fn origin(pack: &LayeredPack, path: &str, kind: OriginKind) -> BlobOrigin {
    BlobOrigin {
        pack: pack.review().pack.clone(),
        release: pack.review().release.clone(),
        layer: Label::new("root").unwrap(),
        path: AssetPath::parse(path).unwrap(),
        review_digest: pack.review().digest().unwrap(),
        kind,
    }
}

#[test]
fn compiled_compatibility_bones_keep_two_oriented_end_faces_and_four_sides() {
    let budget = ByteBudget::new(32 << 20).unwrap();
    {
        let stop = AtomicBool::new(false);
        let cancel = Cancel::new(&stop);
        let definitions = compatibility(&budget, cancel);
        let mut compiler =
            ModelCompiler::new(&definitions, Limits::default(), budget.clone()).unwrap();
        for (axis, component) in [("x", 0), ("z", 1), ("y", 2)] {
            let normalized = compiler
                .compile_state(&bone_state(axis), [-17, -1, 80], 71839, cancel)
                .unwrap();
            assert_eq!(normalized.applications.len(), 1);
            let (application, model) = &normalized.applications[0];
            assert_eq!(model.quads.len(), 6);
            let mut ends = 0;
            let mut sides = 0;
            let mut normals = BTreeSet::new();
            for quad in &model.quads {
                let oriented = oriented_quad(quad, application).unwrap();
                assert!(oriented
                    .points
                    .iter()
                    .flatten()
                    .all(|v| (-1e-9..=1.0 + 1e-9).contains(v)));
                assert!(oriented
                    .uv
                    .iter()
                    .flatten()
                    .all(|v| (0.0..=1.0).contains(v)));
                match oriented.texture.as_str() {
                    "minecraft:block/bone_block_top" => {
                        assert!(oriented.normal[component].abs() > 0.999);
                        normals.insert(if oriented.normal[component] > 0.0 {
                            1
                        } else {
                            -1
                        });
                        ends += 1;
                    }
                    "minecraft:block/bone_block_side" => {
                        assert!(oriented.normal[component].abs() < 1e-6);
                        sides += 1;
                    }
                    other => panic!("unexpected bone image {other}"),
                }
            }
            assert_eq!((ends, sides), (2, 4));
            assert_eq!(normals, BTreeSet::from([-1, 1]));
        }
    }
    assert_eq!(budget.used(), 0);
}

#[test]
fn synthetic_selected_model_precedence_is_independent_of_blockstate_presence() {
    let budget = ByteBudget::new(32 << 20).unwrap();
    {
        let stop = AtomicBool::new(false);
        let cancel = Cancel::new(&stop);
        let _fixture_charge = budget.reserve(1 << 20, cancel).unwrap();
        let png = std::array::from_fn(|_| fixture_png(1, 1, &[255; 4]));
        assert!(png.iter().map(Vec::capacity).sum::<usize>() < 1 << 20);
        for metadata_case in 0..3 {
            let packs = [
                mounted(
                    OriginKind::SelectedPack,
                    metadata_case,
                    true,
                    &png,
                    &budget,
                    cancel,
                ),
                mounted(
                    OriginKind::ExplicitFullPackFallback,
                    0,
                    true,
                    &png,
                    &budget,
                    cancel,
                ),
            ];
            let compatibility = compatibility(&budget, cancel);
            let definitions =
                DefinitionSources::new(&packs[..1], &[], Some(&compatibility)).unwrap();
            let mut compiler =
                ModelCompiler::new(&definitions, Limits::default(), budget.clone()).unwrap();
            for axis in ["x", "y", "z"] {
                let normalized = compiler
                    .compile_state(&bone_state(axis), [-17, -1, 80], 71839, cancel)
                    .unwrap();
                assert_eq!(
                    normalized.state_origin.compatibility.is_some(),
                    metadata_case == 1
                );
                if metadata_case != 1 {
                    assert_eq!(
                        normalized.state_origin.origin,
                        origin(&packs[0], STATE_PATH, OriginKind::SelectedPack)
                    );
                    assert_eq!(normalized.state_origin.sha256, Digest256::of(STATE));
                }
                let model = &normalized.applications[0].1;
                assert_eq!(model.quads.len(), if metadata_case == 2 { 6 } else { 1 });
                assert_eq!(model.origins[0].compatibility.is_some(), metadata_case == 2);
                if metadata_case == 2 {
                    continue;
                }
                assert_eq!(
                    model.origins[0].origin,
                    origin(&packs[0], MODEL_PATH, OriginKind::SelectedPack)
                );
                assert_eq!(model.origins[0].sha256, Digest256::of(MODEL));
                assert_eq!(
                    model.quads[0].texture.as_str(),
                    "minecraft:block/bone_block_top"
                );
                assert!(model.quads[0]
                    .points
                    .iter()
                    .all(|p| (p[2] - 0.5).abs() < 1e-9));
            }
        }
    }
    assert_eq!(budget.used(), 0);
}

#[test]
fn synthetic_images_require_exact_end_art_or_explicit_same_id_fallback() {
    let limits = Limits::default();
    let budget = ByteBudget::new(limits.decoder_scratch_bytes + (32 << 20)).unwrap();
    {
        let stop = AtomicBool::new(false);
        let cancel = Cancel::new(&stop);
        let _fixture_charge = budget.reserve(1 << 20, cancel).unwrap();
        let rgba = [
            [255, 0, 0, 255],
            [0, 255, 0, 255],
            [0, 0, 255, 255],
            [255, 255, 255, 255],
        ];
        let png = rgba.map(|pixels| fixture_png(1, 1, &pixels));
        assert!(png.iter().map(Vec::capacity).sum::<usize>() < 1 << 20);
        let aliases = explicit_aliases("programmerart").unwrap();
        for case in 0..4 {
            let packs = [
                mounted(
                    OriginKind::SelectedPack,
                    0,
                    case == 0,
                    &png,
                    &budget,
                    cancel,
                ),
                mounted(
                    OriginKind::ExplicitFullPackFallback,
                    0,
                    case != 2,
                    &png,
                    &budget,
                    cancel,
                ),
            ];
            let compatibility = compatibility(&budget, cancel);
            let definitions =
                DefinitionSources::new(&packs[..1], &[], Some(&compatibility)).unwrap();
            let mut compiler = ModelCompiler::new(&definitions, limits, budget.clone()).unwrap();
            let normalized = compiler
                .compile_state(&bone_state("x"), [-17, -1, 80], 71839, cancel)
                .unwrap();
            assert_eq!(
                normalized.state_origin.origin,
                origin(&packs[0], STATE_PATH, OriginKind::SelectedPack)
            );
            assert_eq!(
                normalized.applications[0].1.origins[0].origin,
                origin(&packs[0], MODEL_PATH, OriginKind::SelectedPack)
            );
            assert_eq!(normalized.applications[0].1.quads.len(), 1);
            let requests: Vec<_> = ["bone_block_side", "bone_block_top", "coarse_dirt"]
                .iter()
                .map(|name| {
                    request(
                        ResourceId::parse(&format!("minecraft:block/{name}")).unwrap(),
                        &packs[0].review().pack,
                        &aliases,
                        false,
                        (case != 3).then_some(&packs[1].review().pack),
                        false,
                    )
                    .unwrap()
                })
                .collect();
            let importer = TextureImporter::new(&packs, limits, budget.clone()).unwrap();
            let imports = importer.import(&requests, cancel).unwrap();
            let table = MaterialTable {
                medium: None,
                tints: BTreeMap::new(),
                rules: requests
                    .iter()
                    .map(|r| {
                        (
                            r.requirement.id.clone(),
                            TextureRenderRule {
                                alpha: AlphaMode::Opaque,
                                layer: 0,
                                normal_map: None,
                                specular_map: None,
                            },
                        )
                    })
                    .collect(),
            };
            let bound = BoundModel::bind(&normalized, &imports.bank, &table, &budget, cancel);
            assert_eq!(
                bound.is_ok(),
                case < 2,
                "missing end image must not bind side art"
            );
            if let Ok(bound) = bound {
                assert_eq!(bound.quads.len(), 1);
                assert_eq!(
                    Some(bound.quads[0].material.texture),
                    imports
                        .bank
                        .resolve(&ResourceId::parse("minecraft:block/bone_block_top").unwrap())
                );
            }
            for (index, name) in ["bone_block_side", "bone_block_top", "coarse_dirt"]
                .iter()
                .enumerate()
            {
                let id = ResourceId::parse(&format!("minecraft:block/{name}")).unwrap();
                let record = imports.records.iter().find(|r| r.resource == id).unwrap();
                if index == 1 && case >= 2 {
                    assert!(
                        record.failure.is_some()
                            && record.source.is_none()
                            && record.rgba_sha256.is_none()
                    );
                    assert!(imports.bank.resolve(&id).is_none());
                    let row = imports
                        .bank
                        .coverage()
                        .rows
                        .iter()
                        .find(|r| r.requirement.id == id)
                        .unwrap();
                    assert_eq!(row.problems, vec![CoverageProblem::Missing]);
                    continue;
                }
                let fallback = index == 1 && case == 1;
                let selected_pack = usize::from(fallback);
                let kind = if fallback {
                    OriginKind::ExplicitFullPackFallback
                } else {
                    OriginKind::SelectedPack
                };
                let pixel = if fallback {
                    3
                } else if index == 1 {
                    1
                } else {
                    0
                };
                let path = [SIDE_PATH, TOP_PATH, COARSE_PATH][index];
                let expected = origin(&packs[selected_pack], path, kind);
                assert!(record.failure.is_none());
                assert_eq!(record.source.as_ref(), Some(&expected));
                assert_eq!(record.source_sha256, Some(Digest256::of(&png[pixel])));
                assert_eq!(record.rgba_sha256, Some(Digest256::of(&rgba[pixel])));
                let texture = imports
                    .bank
                    .texture(imports.bank.resolve(&id).unwrap())
                    .unwrap();
                assert_eq!(texture.image().origin(), &expected);
                assert_eq!(texture.image().bytes(), rgba[pixel].as_slice());
                if fallback || index == 2 {
                    assert!(record.candidates.last().unwrap().alias_evidence.is_some());
                }
            }
            assert_eq!(
                imports.bank.coverage().selected_texture_count,
                if case == 0 { 3 } else { 2 }
            );
            assert_eq!(
                imports.bank.coverage().explicit_fallback_texture_count,
                usize::from(case == 1)
            );
            stop.store(true, Ordering::Release);
            assert!(matches!(
                importer.import(&requests, cancel),
                Err(AssetError::Cancelled)
            ));
            stop.store(false, Ordering::Release);
        }
    }
    assert_eq!(budget.used(), 0);
}

#[test]
fn material_requests_keep_logical_identity_and_only_evidenced_aliases() {
    let selected = ResourceId::parse("fixture:selected").unwrap();
    let fallback = ResourceId::parse("fixture:fallback").unwrap();
    let coarse = ResourceId::parse("minecraft:block/coarse_dirt").unwrap();
    assert_eq!(
        explicit_aliases("programmerart").unwrap()[&coarse],
        vec![AssetPath::parse(COARSE_PATH).unwrap()]
    );
    for profile in [
        "jicklus",
        "f8thful",
        "whimscape",
        "goodvibes",
        "programmerart",
        "textureless",
        "plasticator",
        "pixelperfectionce",
        "faithful32",
        "faithful64",
        "antumbra",
    ] {
        let aliases = explicit_aliases(profile).unwrap();
        for name in MATERIALS {
            let id = ResourceId::parse(&format!("minecraft:block/{name}")).unwrap();
            let request = request(
                id.clone(),
                &selected,
                &aliases,
                profile == "goodvibes",
                Some(&fallback),
                false,
            )
            .unwrap();
            assert_eq!(request.requirement.id, id);
            for candidate in &request.candidates {
                match &candidate.location {
                    TextureLocation::Resource {
                        id: resource,
                        alias_reason,
                    } => {
                        assert_eq!(
                            resource, &id,
                            "cross-material substitution for {profile}/{name}"
                        );
                        assert_eq!(alias_reason.is_some(), candidate.pack == fallback);
                    }
                    TextureLocation::Literal { path, .. } => {
                        assert_eq!(candidate.pack, selected);
                        assert!(
                            aliases.get(&id).is_some_and(|paths| paths.contains(path))
                                || (profile == "goodvibes"
                                    && path.as_str() == format!("textures/{}.png", id.parts().1))
                        );
                    }
                    TextureLocation::Bedrock { .. } => panic!("unexpected edition substitution"),
                }
            }
            assert_eq!(request.candidates.first().unwrap().pack, selected);
            assert_eq!(request.candidates.last().unwrap().pack, fallback);
        }
    }
}

#[test]
fn legacy_programmerart_terracotta_aliases_precede_other_pack_fallbacks() {
    let selected = ResourceId::parse("fixture:selected").unwrap();
    let fallback = ResourceId::parse("fixture:fallback").unwrap();
    let aliases = explicit_aliases("programmerart").unwrap();
    for color in [
        "black",
        "blue",
        "brown",
        "cyan",
        "gray",
        "green",
        "light_blue",
        "lime",
        "magenta",
        "orange",
        "pink",
        "purple",
        "red",
        "light_gray",
        "white",
        "yellow",
    ] {
        let id = ResourceId::parse(&format!("minecraft:block/{color}_terracotta")).unwrap();
        let old_color = if color == "light_gray" {
            "silver"
        } else {
            color
        };
        let path = AssetPath::parse(&format!(
            "assets/minecraft/textures/blocks/hardened_clay_stained_{old_color}.png"
        ))
        .unwrap();
        assert!(aliases[&id].contains(&path));
        let request = request(
            id.clone(),
            &selected,
            &aliases,
            false,
            Some(&fallback),
            false,
        )
        .unwrap();
        assert_eq!(request.requirement.id, id);
        let alias_index = request.candidates.iter().position(|candidate| candidate.pack == selected && matches!(&candidate.location, TextureLocation::Literal { path: actual, .. } if actual == &path)).unwrap();
        let fallback_index = request
            .candidates
            .iter()
            .position(|candidate| candidate.pack == fallback)
            .unwrap();
        assert!(alias_index < fallback_index);
    }
}
