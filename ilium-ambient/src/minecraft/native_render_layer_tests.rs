use super::*;

#[test]
fn pinned_registration_count_and_default_are_exact() {
    let layers = RenderLayers::load(true).unwrap();
    assert_eq!(layers.blocks.len(), 273);
    assert_eq!(layers.block("minecraft:stone").unwrap(), Layer::Solid);
    assert_eq!(layers.block("minecraft:glass").unwrap(), Layer::Cutout);
    assert_eq!(
        layers.block("minecraft:glass_pane").unwrap(),
        Layer::CutoutMipped
    );
    assert_eq!(layers.block("minecraft:ice").unwrap(), Layer::Translucent);
    assert_eq!(
        layers.block("minecraft:tinted_glass").unwrap(),
        Layer::Translucent
    );
    assert!(matches!(
        layers.block("custom:stone"),
        Err(Error::Namespace)
    ));
}

#[test]
fn fancy_leaves_and_fluids_follow_distinct_native_paths() {
    let fancy = RenderLayers::load(true).unwrap();
    let fast = RenderLayers::load(false).unwrap();
    assert_eq!(
        fancy.block("minecraft:oak_leaves").unwrap(),
        Layer::CutoutMipped
    );
    assert_eq!(fast.block("minecraft:oak_leaves").unwrap(), Layer::Solid);
    assert_eq!(fancy.fluid_water(), Layer::Translucent);
    assert_eq!(fancy.fluid_lava(), Layer::Solid);
}
