//! Original authored surface-entity assemblies. Integration is owned by the scene renderer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Species { Cow }
pub const ALL_SPECIES: &[Species] = &[Species::Cow];
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AtlasLayout { Legacy, Modern, Bedrock }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClimateSkin { Temperate, Warm, Cold }
#[derive(Clone, Debug)]
pub struct Part { pub name: &'static str, pub min: [f32; 3], pub max: [f32; 3] }
#[derive(Clone, Debug)]
pub struct Model { pub parts: Vec<Part> }
pub fn model(_species: Species, _layout: AtlasLayout, _climate: ClimateSkin) -> Model { Model { parts: vec![] } }
#[cfg(test)]
mod tests {
 use super::*;
 #[test] fn cow_has_head_body_horns_and_four_separate_legs() {
  let m=model(Species::Cow,AtlasLayout::Legacy,ClimateSkin::Temperate);
  assert!(m.parts.iter().any(|p|p.name=="head"));
  assert!(m.parts.iter().any(|p|p.name=="body"));
  assert_eq!(m.parts.iter().filter(|p|p.name.starts_with("leg_")).count(),4);
  assert_eq!(m.parts.iter().filter(|p|p.name.starts_with("horn_")).count(),2);
 }
}
