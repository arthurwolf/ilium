//! Compile-time embedded recap prompt catalog.

pub const RECAP: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/recap/recap.hbs"
));

pub(crate) const TEMPLATES: &[(&str, &str)] = &[("recap/recap", RECAP)];
