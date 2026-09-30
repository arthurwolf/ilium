//! The only runtime sources are these compile-time embedded catalogs.

#[path = "naming.rs"]
pub mod naming;
#[path = "voice.rs"]
pub mod voice;
#[path = "agent.rs"]
pub mod agent;
#[path = "conversion.rs"]
pub mod conversion;

pub fn catalog() -> impl Iterator<Item = &'static (&'static str, &'static str)> {
    naming::TEMPLATES
        .iter()
        .chain(voice::TEMPLATES)
        .chain(agent::TEMPLATES)
        .chain(conversion::TEMPLATES)
}
