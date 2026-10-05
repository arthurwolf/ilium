//! Bounded, artwork-free texture foundation. Blocking import adapters belong on
//! the scene's owned worker. Sampling borrows immutable data and reads only the
//! caller's frame time. B1 deliberately does not register selectable UI packs.
#[cfg(test)]
mod adapters_tests;
pub mod animation;
pub mod archive;
pub mod bank;
pub mod bedrock;
pub mod block_state;
pub mod budget;
pub mod compatibility;
pub mod error;
pub mod identity;
pub mod importer;
pub mod layers;
pub mod material_data;
pub mod metadata;
#[cfg(test)]
mod model_tests;
pub mod models;
pub mod pixels;
pub mod review;
pub mod source;
pub mod texture;
mod texture_minification;
pub mod tga;

pub use animation::{AnimationPlan, MissingAnimation, PixelRect};
pub use bank::{TextureBank, TextureBankBuilder, TextureHandle, TextureRequirement};
pub use budget::{ByteBudget, Cancel, Limits};
pub use error::{AssetError, Result};
pub use identity::{AssetPath, BlobOrigin, Digest256, OriginKind, ResourceId, SourceBlob};
pub use pixels::PixelImage;
pub use review::{FullPackReview, PackScope};
pub use texture::{Encoding, LinearRgba, Texture};
