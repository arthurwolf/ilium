//! The only runtime sources are these compile-time embedded catalogs.

#[path = "agent.rs"]
pub mod agent;
#[path = "compaction.rs"]
pub mod compaction;
#[path = "conversion.rs"]
pub mod conversion;
#[path = "naming.rs"]
pub mod naming;
#[path = "voice.rs"]
pub mod voice;

pub fn catalog() -> impl Iterator<Item = &'static (&'static str, &'static str)> {
    naming::TEMPLATES
        .iter()
        .chain(voice::TEMPLATES)
        .chain(agent::TEMPLATES)
        .chain(conversion::TEMPLATES)
        .chain(compaction::TEMPLATES)
}
