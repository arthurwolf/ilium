//! Frame-local provenance bridge. Model face positions use ground-x,
//! ground-y,height; retained Java states use x,height,z.

use super::tours::{self, DisplayedBlock, PreparedMap};
use crate::raster::PaintedOwner;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

const MAX_OWNERS: usize = 8192;

/// Retains the exact decoded palette snapshot used for this raster. The scene
/// also retains its corresponding FrameTag; the controller validates that tag
/// only after a successful host presentation. Retire the last snapshot Arc on
/// the preparation worker, never through a render-time last-owner drop.
pub struct FrameOwners {
    map: Arc<PreparedMap>,
    positions: Vec<[i32; 3]>,
    ids: BTreeMap<[i32; 3], u32>,
}

impl FrameOwners {
    pub fn new(map: Arc<PreparedMap>) -> Self {
        Self {
            map,
            positions: Vec::new(),
            ids: BTreeMap::new(),
        }
    }

    /// `resolved` requires authentic state-bound geometry and selected artwork;
    /// source substitutions and missing-art diagnostics must pass false.
    pub fn register(&mut self, contributors: &[Option<[i32; 3]>], resolved: bool) -> u32 {
        if !resolved || contributors.len() > 9 {
            return 0;
        }
        let Some(position) = single_source(contributors) else {
            return 0;
        };
        if self.map.state(position).is_none_or(|state| state.is_air()) {
            return 0;
        }
        if let Some(&id) = self.ids.get(&position) {
            return id;
        }
        if self.positions.len() >= MAX_OWNERS || self.positions.try_reserve(1).is_err() {
            return 0;
        }
        let id = self.positions.len() as u32 + 1;
        self.positions.push(position);
        self.ids.insert(position, id);
        id
    }

    pub fn displayed(
        &self,
        painted: &[PaintedOwner],
    ) -> Result<Vec<DisplayedBlock<'_>>, tours::Error> {
        if painted.len() > MAX_OWNERS {
            return Err(tours::Error::Limit("painted owners"));
        }
        let mut seen = BTreeSet::new();
        let mut output = Vec::new();
        output
            .try_reserve_exact(painted.len())
            .map_err(|_| tours::Error::Limit("painted owner allocation"))?;
        for owner in painted {
            if owner.id == 0 || !seen.insert(owner.id) {
                return Err(tours::Error::Invalid("zero or duplicate painted owner"));
            }
            let position = *self
                .positions
                .get(owner.id as usize - 1)
                .ok_or(tours::Error::Invalid("unknown painted owner"))?;
            let state = self.map.state(position).ok_or(tours::Error::Invalid(
                "painted state absent from retained snapshot",
            ))?;
            if owner.dots > 0 {
                output.push(DisplayedBlock {
                    position,
                    state,
                    pixels: owner.dots,
                    resolved: true,
                });
            }
        }
        // Integer owner IDs describe registration order; the controller's
        // binary searches require chunk/Java-position order instead.
        output.sort_unstable_by_key(tours::display_order);
        Ok(output)
    }
}

/// This swap is its own inverse and never rotates block-state properties.
pub const fn convert_axes(position: [i32; 3]) -> [i32; 3] {
    [position[0], position[2], position[1]]
}

/// Certify one saved cell only when every surviving alpha contributor belongs
/// to it. Empty or mixed-source pixels must remain unattributed.
pub fn single_source(contributors: &[Option<[i32; 3]>]) -> Option<[i32; 3]> {
    let mut sources = contributors.iter().flatten();
    let position = *sources.next()?;
    sources
        .all(|source| *source == position)
        .then(|| convert_axes(position))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::minecraft::{chunk, evidence, loader, nbt};

    fn map() -> Arc<PreparedMap> {
        use nbt::{Compound, Document, Tag};
        let fields = |values: Vec<(&str, Tag)>| -> Compound {
            values
                .into_iter()
                .map(|(name, value)| (name.into(), value))
                .collect()
        };
        let sections = (-4..=19)
            .map(|y| {
                Tag::Compound(fields(vec![
                    ("Y", Tag::Byte(y)),
                    (
                        "block_states",
                        Tag::Compound(fields(vec![(
                            "palette",
                            Tag::List {
                                kind: 10,
                                values: vec![Tag::Compound(fields(vec![(
                                    "Name",
                                    Tag::String("minecraft:stone".into()),
                                )]))],
                            },
                        )])),
                    ),
                ]))
            })
            .collect();
        let decoded = chunk::decode(
            &Document {
                name: "synthetic receipt fixture".into(),
                root: fields(vec![
                    ("DataVersion", Tag::Int(3218)),
                    ("xPos", Tag::Int(0)),
                    ("zPos", Tag::Int(0)),
                    ("Status", Tag::String("full".into())),
                    (
                        "sections",
                        Tag::List {
                            kind: 10,
                            values: sections,
                        },
                    ),
                ]),
            },
            [0, 0],
            chunk::Limits::default(),
            &|| false,
        )
        .unwrap();
        let mut loaded = loader::LoadedWindow::default();
        loaded.chunks.insert([0, 0], Arc::new(decoded));
        loaded.coverage.chunks.insert([0, 0]);
        Arc::new(
            PreparedMap::new(
                evidence::Source {
                    map: evidence::MapId([1; 16]),
                    generation: 1,
                },
                0,
                Arc::new(loaded),
                Vec::new(),
                &mut tours::Budget::new(u64::MAX, &|| false),
            )
            .unwrap(),
        )
    }

    #[test]
    fn receipts_restore_exact_palette_pointers_and_domain_order() {
        let map = map();
        let mut owners = FrameOwners::new(Arc::clone(&map));
        let first = owners.register(&[Some([15, 15, 70])], true);
        let second = owners.register(&[Some([0, 0, 70])], true);
        assert_ne!(first, 0);
        assert_ne!(second, 0);
        assert_eq!(owners.register(&[Some([15, 15, 70])], true), first);
        let shown = owners
            .displayed(&[
                PaintedOwner { id: first, dots: 3 },
                PaintedOwner {
                    id: second,
                    dots: 8,
                },
            ])
            .unwrap();
        assert_eq!(
            shown.iter().map(|block| block.position).collect::<Vec<_>>(),
            [[0, 70, 0], [15, 70, 15]]
        );
        assert_eq!(shown[0].pixels, 8);
        assert!(shown
            .iter()
            .all(|block| block.resolved
                && std::ptr::eq(block.state, map.state(block.position).unwrap())));
    }

    #[test]
    fn unresolved_mixed_missing_and_forged_owners_cannot_be_credited() {
        let mut owners = FrameOwners::new(map());
        assert_eq!(owners.register(&[Some([0, 0, 70])], false), 0);
        assert_eq!(
            owners.register(&[Some([0, 0, 70]), Some([1, 0, 70])], true),
            0
        );
        assert_eq!(owners.register(&[Some([16, 0, 70])], true), 0);
        assert!(owners
            .displayed(&[PaintedOwner { id: 123, dots: 2 }])
            .is_err());
        let id = owners.register(&[Some([0, 0, 70])], true);
        assert_ne!(id, 0);
        assert!(owners
            .displayed(&[PaintedOwner { id, dots: 2 }, PaintedOwner { id, dots: 1 }])
            .is_err());
        assert!(owners
            .displayed(&[PaintedOwner { id, dots: 0 }])
            .unwrap()
            .is_empty());
    }

    #[test]
    fn owner_cap_withholds_new_ids_without_invalidating_existing_receipts() {
        let mut owners = FrameOwners::new(map());
        for index in 0..MAX_OWNERS {
            let position = [
                (index % 16) as i32,
                ((index / 16) % 16) as i32,
                -64 + (index / 256) as i32,
            ];
            assert_eq!(owners.register(&[Some(position)], true), index as u32 + 1);
        }
        assert_eq!(owners.register(&[Some([0, 0, -32])], true), 0);
        assert_eq!(owners.register(&[Some([0, 0, -64])], true), 1);
        let shown = owners
            .displayed(&[PaintedOwner { id: 1, dots: 3 }])
            .unwrap();
        assert_eq!(shown[0].position, [0, -64, 0]);
        assert!(matches!(
            owners.displayed(&vec![PaintedOwner { id: 1, dots: 1 }; MAX_OWNERS + 1]),
            Err(tours::Error::Limit(_))
        ));
    }

    #[test]
    fn axes_preserve_negative_height_and_signed_ground_coordinates() {
        let java = [-17, -64, 1041];
        assert_eq!(convert_axes(java), [-17, 1041, -64]);
        assert_eq!(convert_axes(convert_axes(java)), java);
        assert_eq!(
            convert_axes([i32::MIN, 319, i32::MAX]),
            [i32::MIN, i32::MAX, 319]
        );
    }

    #[test]
    fn multiple_faces_of_same_cell_keep_exact_java_owner() {
        assert_eq!(
            single_source(&[None, Some([-17, 1041, -64]), Some([-17, 1041, -64]), None]),
            Some([-17, -64, 1041])
        );
    }

    #[test]
    fn empty_and_distinct_translucent_sources_cannot_earn_credit() {
        assert_eq!(single_source(&[]), None);
        assert_eq!(single_source(&[None; 9]), None);
        assert_eq!(single_source(&[Some([1, 2, 3]), Some([1, 2, 4])]), None);
        assert_eq!(
            single_source(&[Some([1, 2, 3]), None, Some([4, 2, 3])]),
            None
        );
    }
}
