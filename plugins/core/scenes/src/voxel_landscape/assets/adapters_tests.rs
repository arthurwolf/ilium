//! Original synthetic end-to-end source/overlay/TGA/Bedrock fixtures, never selected-pack artwork proof.
use super::{
    animation::{AnimationEvidence, MissingAnimation},
    archive::{synthetic_zip, ZipSource},
    bank::TextureRequirement,
    bedrock::{
        BedrockIndex, EditionTextureBinding, FlipbookDefaults, FlipbookTable, TerrainTextureTable,
    },
    budget::{ByteBudget, Cancel, Limits},
    identity::{AssetPath, Digest256, Label, OriginKind, ResourceId, SourceBlob},
    importer::{
        ScheduleSource, TextureCandidate, TextureImporter, TextureLocation, TextureRequest,
    },
    layers::{LayeredPack, MountLayout, PackFormat},
    metadata::Document,
    pixels::{fixture_png, ImageExpectations},
    review::{fixture_origin, fixture_review, SourceEdition},
    source::{SourceBytes, SourceLimits},
};
use std::{
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};
fn mounted(
    entries: &[(String, Vec<u8>)],
    layout: MountLayout,
    root: Option<&str>,
    budget: &ByteBudget,
    cancel: Cancel<'_>,
) -> LayeredPack {
    let borrowed: Vec<_> = entries
        .iter()
        .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
        .collect();
    let bytes = synthetic_zip(&borrowed, true, true);
    let source = ZipSource::open(
        SourceBytes::from_slice(&bytes, 256 << 20, budget, cancel).unwrap(),
        Some(Digest256::of(&bytes)),
        SourceLimits::default(),
        budget,
        cancel,
    )
    .unwrap();
    let mut review = fixture_review();
    review.edition = match layout {
        MountLayout::Java => SourceEdition::Java,
        MountLayout::ExtractedJava => SourceEdition::ExtractedWorldArt,
        MountLayout::Bedrock => SourceEdition::Bedrock,
    };
    LayeredPack::mount(
        review,
        Arc::new(source),
        root.map(|v| AssetPath::parse(v).unwrap()),
        layout,
        OriginKind::DiagnosticFixture,
        Limits::default(),
        budget.clone(),
        cancel,
    )
    .unwrap()
}
fn metadata(value: serde_json::Value, budget: &ByteBudget, cancel: Cancel<'_>) -> Document {
    let blob = SourceBlob::new(
        serde_json::to_vec(&value).unwrap(),
        fixture_origin(OriginKind::DiagnosticFixture),
        None,
        &Limits::default(),
        budget,
        cancel,
    )
    .unwrap();
    Document::parse(&blob, &Limits::default(), budget, cancel).unwrap()
}
#[test]
fn seventy_cumulative_overlays_keep_authored_order_minor_formats_and_effective_origin() {
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let budget = ByteBudget::new(128 << 20).unwrap();
    let mut overlays = Vec::new();
    let mut entries = vec![
        (
            "pack/assets/minecraft/textures/block/probe.png".into(),
            b"root".to_vec(),
        ),
        (
            "pack/assets/minecraft/textures/block/base.png".into(),
            b"base-only".to_vec(),
        ),
    ];
    for index in 0..70 {
        let directory = format!("v{:03}", 70 - index);
        overlays.push(serde_json::json!({"directory":directory,"formats":[15,84],"min_format":[70,1],"max_format":[84,0]}));
        entries.push((
            format!("pack/{directory}/assets/minecraft/textures/block/probe.png"),
            vec![index as u8],
        ));
    }
    entries.push(("pack/pack.mcmeta".into(),serde_json::to_vec(&serde_json::json!({"pack":{"pack_format":15,"supported_formats":[15,84],"min_format":[15,0],"max_format":[84,0]},"overlays":{"entries":overlays}})).unwrap()));
    let pack = mounted(&entries, MountLayout::Java, Some("pack"), &budget, cancel)
        .with_java_overlays(PackFormat::new(70, 1).unwrap(), None, cancel)
        .unwrap();
    assert_eq!(pack.layer_count(), 71);
    let result = pack
        .read_path(
            &AssetPath::parse("assets/minecraft/textures/block/probe.png").unwrap(),
            64,
            cancel,
        )
        .unwrap();
    let blob = result.blob.unwrap();
    assert_eq!(blob.bytes(), [69]);
    assert!(blob.origin().layer.as_str().starts_with("overlay[69]:v001"));
    assert_eq!(result.target, Some(PackFormat::new(70, 1).unwrap()));
    let base = pack
        .read_path(
            &AssetPath::parse("assets/minecraft/textures/block/base.png").unwrap(),
            64,
            cancel,
        )
        .unwrap();
    assert_eq!(base.blob.unwrap().bytes(), b"base-only");
    assert_eq!(base.attempts.len(), 71);
    let root_only = mounted(&entries, MountLayout::Java, Some("pack"), &budget, cancel)
        .with_java_overlays(PackFormat::new(70, 0).unwrap(), None, cancel)
        .unwrap();
    assert_eq!(root_only.layer_count(), 1);
    assert!(
        mounted(&entries, MountLayout::Java, Some("pack"), &budget, cancel)
            .with_java_overlays(PackFormat::new(84, 1).unwrap(), None, cancel)
            .is_err()
    );
}
#[test]
fn nested_bedrock_manifest_without_inherited_tables_does_not_fabricate_models_or_timings() {
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let budget = ByteBudget::new(32 << 20).unwrap();
    let manifest = serde_json::json!({"format_version":1,"header":{"version":[2,4,0]},"modules":[{"type":"resources"}]});
    let entries = vec![(
        "Plasticator Texture Pack/manifest.json".into(),
        serde_json::to_vec(&manifest).unwrap(),
    )];
    let pack = mounted(
        &entries,
        MountLayout::Bedrock,
        Some("Plasticator Texture Pack"),
        &budget,
        cancel,
    );
    let index = BedrockIndex::load(&pack, cancel).unwrap();
    assert_eq!(index.manifest.version, [2, 4, 0]);
    assert!(index.terrain.is_none());
    assert!(index.flipbooks.is_none());
    assert_eq!(index.missing_inherited_tables.len(), 2);
}
#[test]
fn tga_foliage_reaches_the_bank_through_explicit_bedrock_alias_without_native_java_claim() {
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let budget = ByteBudget::new(192 << 20).unwrap();
    let mut tga = vec![0u8; 18];
    tga[2] = 2;
    tga[12] = 2;
    tga[14] = 1;
    tga[16] = 32;
    tga[17] = 40;
    tga.extend([10, 20, 30, 0, 40, 50, 60, 153]);
    let original_hash = Digest256::of(&tga);
    let entries=vec![("Pack/textures/blocks/fern.tga".into(),tga),("Pack/manifest.json".into(),serde_json::to_vec(&serde_json::json!({"format_version":1,"header":{"version":[2,4,0]},"modules":[{"type":"resources"}]})).unwrap())];
    let pack = mounted(
        &entries,
        MountLayout::Bedrock,
        Some("Pack"),
        &budget,
        cancel,
    );
    let pack_id = pack.review().pack.clone();
    let id = ResourceId::parse("minecraft:block/fern").unwrap();
    let candidate = TextureCandidate {
        pack: pack_id,
        location: TextureLocation::Bedrock {
            binding: EditionTextureBinding {
                semantic: id.clone(),
                source_path: AssetPath::parse("textures/blocks/fern").unwrap(),
                evidence: Label::new("Synthetic explicit Bedrock fern-to-semantic binding")
                    .unwrap(),
                extensions: vec!["png".into(), "tga".into()],
            },
        },
        schedule: ScheduleSource::NoMetadata,
        expected_source_sha256: Some(original_hash),
        expected_image: ImageExpectations {
            dimensions: Some([2, 1]),
            rgba_sha256: None,
        },
    };
    let request = TextureRequest {
        requirement: TextureRequirement::selected_color(id.clone()),
        candidates: vec![candidate],
        missing_animation: MissingAnimation::StaticImage,
    };
    let packs = [pack];
    let mut progress = Vec::new();
    let output = TextureImporter::new(&packs, Limits::default(), budget.clone())
        .unwrap()
        .import_with_progress(
            &[request],
            cancel,
            &mut |completed, total, resource, loading| {
                progress.push((completed, total, resource.to_string(), loading));
            },
        )
        .unwrap();
    assert_eq!(
        progress,
        [
            (0, 1, "minecraft:block/fern".into(), true),
            (1, 1, "minecraft:block/fern".into(), false),
        ],
        "texture import should report the current asset and each completed request"
    );
    assert_eq!(output.bank.len(), 1);
    assert!(!output.bank.coverage().required_textures_satisfied);
    assert_eq!(output.bank.coverage().diagnostic_texture_count, 1);
    assert_eq!(output.bank.coverage().selected_texture_count, 0);
    let texture = output
        .bank
        .texture(output.bank.resolve(&id).unwrap())
        .unwrap();
    assert_eq!(texture.image().pixel(1, 0), Some([60, 50, 40, 153]));
    assert_eq!(texture.image().source_sha256(), original_hash);
    assert_eq!(output.records[0].candidates[0].attempts.len(), 2);
    assert_eq!(
        output.bank.selected_review().edition,
        SourceEdition::Bedrock
    );
    assert!(output.records[0].candidates[0].alias_evidence.is_some());
    assert!(!texture.animation().authored_schedule());
}
#[test]
fn bedrock_variant_arrays_and_repeated_frame_sequences_are_explicit_not_filename_guesses() {
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let budget = ByteBudget::new(32 << 20).unwrap();
    let terrain=TerrainTextureTable::parse(metadata(serde_json::json!({"texture_data":{"plant":{"textures":["textures/blocks/young",{"path":"textures/blocks/mature","tint_color":"#abcdef"}]}}}),&budget,cancel)).unwrap();
    assert_eq!(
        terrain.variant("plant", 1).unwrap().path.as_str(),
        "textures/blocks/mature"
    );
    assert!(terrain.variant("absent", 0).is_err());
    let table=FlipbookTable::parse(metadata(serde_json::json!([{"flipbook_texture":"textures/blocks/water","atlas_tile":"water","ticks_per_frame":2,"frames":[1,0,1],"blend_frames":false}]),&budget,cancel)).unwrap();
    let defaults = FlipbookDefaults {
        ticks: 7,
        blend: true,
        evidence: Label::new("Synthetic explicitly supplied default policy").unwrap(),
    };
    let plan = table
        .plan(0, [4, 8], &defaults, &Limits::default(), &budget, cancel)
        .unwrap();
    assert_eq!(plan.at(Duration::ZERO).current.y, 4);
    assert_eq!(plan.at(Duration::from_millis(100)).current.y, 0);
    assert_eq!(plan.at(Duration::from_millis(200)).current.y, 4);
    assert!(!plan.authored_schedule());
    assert!(matches!(
        plan.evidence(),
        AnimationEvidence::BedrockMetadata {
            defaults: None,
            default_sequence: false,
            ..
        }
    ));
    assert!(table
        .plan(
            0,
            [1021, 16384],
            &defaults,
            &Limits::default(),
            &budget,
            cancel
        )
        .is_err());
}
#[test]
fn malformed_unrequested_gui_metadata_does_not_reject_valid_surface_pixels() {
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let budget = ByteBudget::new(192 << 20).unwrap();
    let png = fixture_png(1, 1, &[90, 80, 70, 255]);
    let hash = Digest256::of(&png);
    let entries = vec![
        ("assets/minecraft/textures/block/stone.png".into(), png),
        (
            "assets/minecraft/textures/gui/bad.png.mcmeta".into(),
            b"{} {}".to_vec(),
        ),
    ];
    let pack = mounted(&entries, MountLayout::Java, None, &budget, cancel);
    let id = ResourceId::parse("block/stone").unwrap();
    let candidate = TextureCandidate {
        pack: pack.review().pack.clone(),
        location: TextureLocation::Resource {
            id: id.clone(),
            alias_reason: None,
        },
        schedule: ScheduleSource::AutomaticJava,
        expected_source_sha256: Some(hash),
        expected_image: ImageExpectations::default(),
    };
    let request = TextureRequest {
        requirement: TextureRequirement::selected_color(id.clone()),
        candidates: vec![candidate],
        missing_animation: MissingAnimation::StaticImage,
    };
    let packs = [pack];
    let output = TextureImporter::new(&packs, Limits::default(), budget.clone())
        .unwrap()
        .import(&[request], cancel)
        .unwrap();
    assert!(output.records[0].failure.is_none());
    assert!(output.bank.resolve(&id).is_some());
    assert!(!output.bank.coverage().required_textures_satisfied);
}
#[test]
fn missing_resources_remain_visible_in_the_report() {
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let budget = ByteBudget::new(32 << 20).unwrap();
    let entries = vec![("pack.mcmeta".into(), b"{}".to_vec())];
    let pack = mounted(&entries, MountLayout::Java, None, &budget, cancel);
    let id = ResourceId::parse("block/needed").unwrap();
    let candidate = TextureCandidate {
        pack: pack.review().pack.clone(),
        location: TextureLocation::Resource {
            id: id.clone(),
            alias_reason: None,
        },
        schedule: ScheduleSource::AutomaticJava,
        expected_source_sha256: None,
        expected_image: ImageExpectations::default(),
    };
    let request = TextureRequest {
        requirement: TextureRequirement::selected_color(id),
        candidates: vec![candidate],
        missing_animation: MissingAnimation::StaticImage,
    };
    let packs = [pack];
    let output = TextureImporter::new(&packs, Limits::default(), budget.clone())
        .unwrap()
        .import(&[request], cancel)
        .unwrap();
    assert_eq!(output.bank.len(), 0);
    assert!(output.records[0].failure.is_some());
    assert!(!output.records[0].candidates[0].attempts[0].found);
    assert_eq!(output.bank.coverage().required_satisfied, 0);
}
#[test]
fn extracted_tree_target_is_labeled_compatibility_without_authored_overlay_claims() {
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let budget = ByteBudget::new(16 << 20).unwrap();
    let entries = vec![(
        "minecraft/textures/block/stone.png".into(),
        b"not-decoded-in-mount-test".to_vec(),
    )];
    let pack = mounted(&entries, MountLayout::ExtractedJava, None, &budget, cancel)
        .with_external_target(
            PackFormat::new(84, 0).unwrap(),
            Label::new("Synthetic extracted-art target mapping, not authored pack metadata")
                .unwrap(),
            cancel,
        )
        .unwrap();
    assert_eq!(pack.layer_count(), 1);
    assert!(pack.compatibility().is_some());
    assert_eq!(pack.target(), Some(PackFormat::new(84, 0).unwrap()));
    assert!(pack
        .with_external_target(
            PackFormat::new(84, 0).unwrap(),
            Label::new("Repeated target").unwrap(),
            cancel
        )
        .is_err());
}
#[test]
fn official_addon_root_then_its_overlays_follow_all_base_layers() {
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let budget = ByteBudget::new(32 << 20).unwrap();
    let base=vec![("pack.mcmeta".into(),br#"{"pack":{"pack_format":70},"overlays":{"entries":[{"directory":"base_overlay","formats":70}]}}"#.to_vec()),
        ("assets/minecraft/textures/block/probe.png".into(),b"base".to_vec()),
        ("base_overlay/assets/minecraft/textures/block/probe.png".into(),b"base-overlay".to_vec())];
    let addon=[("addon/pack.mcmeta",br#"{"pack":{"pack_format":70},"overlays":{"entries":[{"directory":"late","formats":70}]}}"#.as_slice()),
        ("addon/assets/minecraft/textures/block/probe.png",b"addon-root".as_slice()),
        ("addon/late/assets/minecraft/textures/block/probe.png",b"addon-overlay".as_slice())];
    let bytes = synthetic_zip(&addon, true, true);
    let source = ZipSource::open(
        SourceBytes::from_slice(&bytes, 256 << 20, &budget, cancel).unwrap(),
        Some(Digest256::of(&bytes)),
        SourceLimits::default(),
        &budget,
        cancel,
    )
    .unwrap();
    let pack = mounted(&base, MountLayout::Java, None, &budget, cancel)
        .with_java_overlays(PackFormat::new(70, 1).unwrap(), None, cancel)
        .unwrap()
        .with_internal_pack(
            Arc::new(source),
            Some(AssetPath::parse("addon").unwrap()),
            Label::new("official-addon-fixture").unwrap(),
            None,
            cancel,
        )
        .unwrap();
    let result = pack
        .read_path(
            &AssetPath::parse("assets/minecraft/textures/block/probe.png").unwrap(),
            64,
            cancel,
        )
        .unwrap();
    assert_eq!(pack.layer_count(), 4);
    let blob = result.blob.unwrap();
    assert_eq!(blob.bytes(), b"addon-overlay");
    assert_eq!(
        blob.origin().layer.as_str(),
        "official-addon-fixture/overlay[0]:late"
    );
    assert!(pack.internal_compatibility().is_empty());
}
#[test] // Verify early shared-budget and alias admission guards with original synthetic data.
fn source_budget_and_alias_evidence_are_checked_before_bank_publication() {
    // Keep these failures distinct from missing resource fallback.
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let budget = ByteBudget::new(16 << 20).unwrap(); // Use one fixture account.
    let entries = vec![("pack.mcmeta".into(), b"{}".to_vec())];
    let pack = mounted(&entries, MountLayout::Java, None, &budget, cancel);
    let packs = [pack]; // Mount only a synthetic source.
    assert!(TextureImporter::new(
        &packs,
        Limits::default(),
        ByteBudget::new(16 << 20).unwrap()
    )
    .is_err()); // A distinct equal-sized account cannot evade the shared cap.
    let request = TextureRequest {
        requirement: TextureRequirement::selected_color(ResourceId::parse("block/needed").unwrap()), // Preserve the requested semantic identity.
        candidates: vec![TextureCandidate {
            pack: packs[0].review().pack.clone(),
            location: TextureLocation::Resource {
                id: ResourceId::parse("block/renamed").unwrap(),
                alias_reason: None,
            }, // Deliberately omit the required rename evidence.
            schedule: ScheduleSource::NoMetadata,
            expected_source_sha256: None,
            expected_image: ImageExpectations::default(),
        }],
        missing_animation: MissingAnimation::StaticImage,
    }; // Do not assert authored animation.
    let result = TextureImporter::new(&packs, Limits::default(), budget.clone())
        .unwrap()
        .import(&[request], cancel)
        .unwrap(); // Retain the resource failure in a non-ready bank report.
    assert!(result.records[0]
        .failure
        .as_deref()
        .unwrap()
        .contains("alias evidence"));
    assert!(result.bank.is_empty()); // An undocumented alias never inserts substitute pixels.
} // Complete the admission regression.
