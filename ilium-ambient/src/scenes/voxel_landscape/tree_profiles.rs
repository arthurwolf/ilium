//! Source-grounded resource and size facts; silhouette construction is an original homage.
//! Frozen Java26.3 source hashes are retained per configuration. Decoration IDs
//! identify requirements, not completed placement. Mushroom size is authored.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TreeResource {
    pub id: &'static str,
    pub properties: &'static [(&'static str, &'static str)],
    pub weight: u16,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeSize {
    SourceTrunk {
        base: u8,
        random_a: u8,
        random_b: u8,
    },
    SourceFallenLength {
        minimum: u8,
        maximum: u8,
    },
    AuthoredMushroomHeight {
        minimum: u8,
        maximum: u8,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeShape {
    Rounded,
    Branched,
    Forked,
    Cherry,
    Dense,
    Spruce,
    Pine,
    GiantSpruce,
    GiantPine,
    GiantJungle,
    Bush,
    Mangrove,
    Poplar,
    Azalea,
    RedMushroom,
    BrownMushroom,
    Fallen,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TreeProfile {
    pub id: &'static str,
    pub source_sha256: &'static str,
    pub stem: TreeResource,
    pub crowns: &'static [TreeResource],
    pub size: TreeSize,
    pub shape: TreeShape,
    pub decorator_ids: &'static [&'static str],
}
pub fn profile(id: &str) -> Option<&'static TreeProfile> {
    TREE_PROFILES.iter().find(|profile| profile.id == id)
}
pub const TREE_PROFILES: [TreeProfile; 44] = [
    TreeProfile {
        id: "minecraft:acacia",
        source_sha256: "0c26032516349b0eb00676046da3ed1b421a70080a07bf46dcd78a6893729455",
        stem: TreeResource {
            id: "minecraft:acacia_log",
            properties: &[("axis", "y")],
            weight: 1,
        },
        crowns: &[TreeResource {
            id: "minecraft:acacia_leaves",
            properties: &[
                ("distance", "7"),
                ("persistent", "false"),
                ("waterlogged", "false"),
            ],
            weight: 1,
        }],
        size: TreeSize::SourceTrunk {
            base: 5,
            random_a: 2,
            random_b: 2,
        },
        shape: TreeShape::Forked,
        decorator_ids: &[],
    },
    TreeProfile {
        id: "minecraft:birch_bees_0002",
        source_sha256: "3084b2aa98ab37943ad77674038a32f094f15f248646199684bba3eb1bf4e763",
        stem: TreeResource {
            id: "minecraft:birch_log",
            properties: &[("axis", "y")],
            weight: 1,
        },
        crowns: &[TreeResource {
            id: "minecraft:birch_leaves",
            properties: &[
                ("distance", "7"),
                ("persistent", "false"),
                ("waterlogged", "false"),
            ],
            weight: 1,
        }],
        size: TreeSize::SourceTrunk {
            base: 5,
            random_a: 2,
            random_b: 0,
        },
        shape: TreeShape::Rounded,
        decorator_ids: &["minecraft:beehive"],
    },
    TreeProfile {
        id: "minecraft:birch_bees_0002_leaf_litter",
        source_sha256: "d73f6344b7bcebf21d9a07cc550fc32dd7612b45e3711570ae17e0fee40840fe",
        stem: TreeResource {
            id: "minecraft:birch_log",
            properties: &[("axis", "y")],
            weight: 1,
        },
        crowns: &[TreeResource {
            id: "minecraft:birch_leaves",
            properties: &[
                ("distance", "7"),
                ("persistent", "false"),
                ("waterlogged", "false"),
            ],
            weight: 1,
        }],
        size: TreeSize::SourceTrunk {
            base: 5,
            random_a: 2,
            random_b: 0,
        },
        shape: TreeShape::Rounded,
        decorator_ids: &["minecraft:beehive", "minecraft:place_on_ground"],
    },
    TreeProfile {
        id: "minecraft:birch_bees_002",
        source_sha256: "5a72ce597d742662344abe963ba95c477aea50f4d3566d97ab1a95b27ab48be1",
        stem: TreeResource {
            id: "minecraft:birch_log",
            properties: &[("axis", "y")],
            weight: 1,
        },
        crowns: &[TreeResource {
            id: "minecraft:birch_leaves",
            properties: &[
                ("distance", "7"),
                ("persistent", "false"),
                ("waterlogged", "false"),
            ],
            weight: 1,
        }],
        size: TreeSize::SourceTrunk {
            base: 5,
            random_a: 2,
            random_b: 0,
        },
        shape: TreeShape::Rounded,
        decorator_ids: &["minecraft:beehive"],
    },
    TreeProfile {
        id: "minecraft:birch_leaf_litter",
        source_sha256: "749c831483275348a4ec76ac02550bf6a41f85212c307998d582bb057d9b8ea8",
        stem: TreeResource {
            id: "minecraft:birch_log",
            properties: &[("axis", "y")],
            weight: 1,
        },
        crowns: &[TreeResource {
            id: "minecraft:birch_leaves",
            properties: &[
                ("distance", "7"),
                ("persistent", "false"),
                ("waterlogged", "false"),
            ],
            weight: 1,
        }],
        size: TreeSize::SourceTrunk {
            base: 5,
            random_a: 2,
            random_b: 0,
        },
        shape: TreeShape::Rounded,
        decorator_ids: &["minecraft:place_on_ground"],
    },
    TreeProfile {
        id: "minecraft:cherry_bees_005",
        source_sha256: "92169563f596960936ee709eea96ddb8e0946098674a09202896cd1e6154a902",
        stem: TreeResource {
            id: "minecraft:cherry_log",
            properties: &[("axis", "y")],
            weight: 1,
        },
        crowns: &[TreeResource {
            id: "minecraft:cherry_leaves",
            properties: &[
                ("distance", "7"),
                ("persistent", "false"),
                ("waterlogged", "false"),
            ],
            weight: 1,
        }],
        size: TreeSize::SourceTrunk {
            base: 7,
            random_a: 1,
            random_b: 0,
        },
        shape: TreeShape::Cherry,
        decorator_ids: &["minecraft:beehive"],
    },
    TreeProfile {
        id: "minecraft:dark_oak_leaf_litter",
        source_sha256: "71f2faef5fb379ca48933aa0a3e775fc1d609e30f75d7155e630b7f0c3eff030",
        stem: TreeResource {
            id: "minecraft:dark_oak_log",
            properties: &[("axis", "y")],
            weight: 1,
        },
        crowns: &[TreeResource {
            id: "minecraft:dark_oak_leaves",
            properties: &[
                ("distance", "7"),
                ("persistent", "false"),
                ("waterlogged", "false"),
            ],
            weight: 1,
        }],
        size: TreeSize::SourceTrunk {
            base: 6,
            random_a: 2,
            random_b: 1,
        },
        shape: TreeShape::Dense,
        decorator_ids: &["minecraft:place_on_ground"],
    },
    TreeProfile {
        id: "minecraft:fallen_birch_tree",
        source_sha256: "3f8a407614c7b5b5b7061b4bcd7e26aa76be0fb8a554e6b7a27d8b718a2789c3",
        stem: TreeResource {
            id: "minecraft:birch_log",
            properties: &[("axis", "y")],
            weight: 1,
        },
        crowns: &[],
        size: TreeSize::SourceFallenLength {
            minimum: 5,
            maximum: 8,
        },
        shape: TreeShape::Fallen,
        decorator_ids: &["minecraft:attached_to_logs"],
    },
    TreeProfile {
        id: "minecraft:fallen_jungle_tree",
        source_sha256: "9f0cf56b3d3bd8aa1b16878f14c9df6dbfe4ec708542b867e43fab1eafe23aad",
        stem: TreeResource {
            id: "minecraft:jungle_log",
            properties: &[("axis", "y")],
            weight: 1,
        },
        crowns: &[],
        size: TreeSize::SourceFallenLength {
            minimum: 4,
            maximum: 11,
        },
        shape: TreeShape::Fallen,
        decorator_ids: &["minecraft:attached_to_logs", "minecraft:trunk_vine"],
    },
    TreeProfile {
        id: "minecraft:fallen_oak_tree",
        source_sha256: "11357d6e725e3820b0813e2f74e4eba330543e4e3e8670dc16db1178c983b13f",
        stem: TreeResource {
            id: "minecraft:oak_log",
            properties: &[("axis", "y")],
            weight: 1,
        },
        crowns: &[],
        size: TreeSize::SourceFallenLength {
            minimum: 4,
            maximum: 7,
        },
        shape: TreeShape::Fallen,
        decorator_ids: &["minecraft:attached_to_logs", "minecraft:trunk_vine"],
    },
    TreeProfile {
        id: "minecraft:fallen_poplar_tree",
        source_sha256: "1d5c3e0d7a7e42007d716b43c58a3feea65298a3de7776560c3f7940b17afe6d",
        stem: TreeResource {
            id: "minecraft:poplar_log",
            properties: &[("axis", "y")],
            weight: 1,
        },
        crowns: &[],
        size: TreeSize::SourceFallenLength {
            minimum: 4,
            maximum: 7,
        },
        shape: TreeShape::Fallen,
        decorator_ids: &["minecraft:attached_to_logs", "minecraft:shelf_mushroom"],
    },
    TreeProfile {
        id: "minecraft:fallen_spruce_tree",
        source_sha256: "e56e44c2ba149577d52190177c606a66bdef445908b9803ce502bcc08460cf6c",
        stem: TreeResource {
            id: "minecraft:spruce_log",
            properties: &[("axis", "y")],
            weight: 1,
        },
        crowns: &[],
        size: TreeSize::SourceFallenLength {
            minimum: 6,
            maximum: 10,
        },
        shape: TreeShape::Fallen,
        decorator_ids: &["minecraft:attached_to_logs"],
    },
    TreeProfile {
        id: "minecraft:fallen_super_birch_tree",
        source_sha256: "c042455de22f2797da2bfde3e51db8a8d2bf6fc67217da3c0dd5c1de89e3dd49",
        stem: TreeResource {
            id: "minecraft:birch_log",
            properties: &[("axis", "y")],
            weight: 1,
        },
        crowns: &[],
        size: TreeSize::SourceFallenLength {
            minimum: 5,
            maximum: 15,
        },
        shape: TreeShape::Fallen,
        decorator_ids: &["minecraft:attached_to_logs"],
    },
    TreeProfile {
        id: "minecraft:fancy_oak",
        source_sha256: "85495119cbe5d043217c87c84239188a91e75894f217d4d17723197cc0312f8c",
        stem: TreeResource {
            id: "minecraft:oak_log",
            properties: &[("axis", "y")],
            weight: 1,
        },
        crowns: &[TreeResource {
            id: "minecraft:oak_leaves",
            properties: &[
                ("distance", "7"),
                ("persistent", "false"),
                ("waterlogged", "false"),
            ],
            weight: 1,
        }],
        size: TreeSize::SourceTrunk {
            base: 3,
            random_a: 11,
            random_b: 0,
        },
        shape: TreeShape::Branched,
        decorator_ids: &[],
    },
    TreeProfile {
        id: "minecraft:fancy_oak_bees",
        source_sha256: "b635da388a44968a647bbbb14af7b124828ddd775c1e9fb64d2592db90912534",
        stem: TreeResource {
            id: "minecraft:oak_log",
            properties: &[("axis", "y")],
            weight: 1,
        },
        crowns: &[TreeResource {
            id: "minecraft:oak_leaves",
            properties: &[
                ("distance", "7"),
                ("persistent", "false"),
                ("waterlogged", "false"),
            ],
            weight: 1,
        }],
        size: TreeSize::SourceTrunk {
            base: 3,
            random_a: 11,
            random_b: 0,
        },
        shape: TreeShape::Branched,
        decorator_ids: &["minecraft:beehive"],
    },
    TreeProfile {
        id: "minecraft:fancy_oak_bees_0002_leaf_litter",
        source_sha256: "bcbd6e0e1e6f854de78187e5d5e69ebcf2ba592eb19b7ba7c0ec2ae88951ece0",
        stem: TreeResource {
            id: "minecraft:oak_log",
            properties: &[("axis", "y")],
            weight: 1,
        },
        crowns: &[TreeResource {
            id: "minecraft:oak_leaves",
            properties: &[
                ("distance", "7"),
                ("persistent", "false"),
                ("waterlogged", "false"),
            ],
            weight: 1,
        }],
        size: TreeSize::SourceTrunk {
            base: 3,
            random_a: 11,
            random_b: 0,
        },
        shape: TreeShape::Branched,
        decorator_ids: &["minecraft:beehive", "minecraft:place_on_ground"],
    },
    TreeProfile {
        id: "minecraft:fancy_oak_bees_002",
        source_sha256: "652d92798708ea07cec921258fabc1ec41554eb94c14574695a61d971914ac13",
        stem: TreeResource {
            id: "minecraft:oak_log",
            properties: &[("axis", "y")],
            weight: 1,
        },
        crowns: &[TreeResource {
            id: "minecraft:oak_leaves",
            properties: &[
                ("distance", "7"),
                ("persistent", "false"),
                ("waterlogged", "false"),
            ],
            weight: 1,
        }],
        size: TreeSize::SourceTrunk {
            base: 3,
            random_a: 11,
            random_b: 0,
        },
        shape: TreeShape::Branched,
        decorator_ids: &["minecraft:beehive"],
    },
    TreeProfile {
        id: "minecraft:fancy_oak_bees_005",
        source_sha256: "eb0edb9a84838dda7aa40f9a59cfea4198f25ae4952873198cd02af692d5f30f",
        stem: TreeResource {
            id: "minecraft:oak_log",
            properties: &[("axis", "y")],
            weight: 1,
        },
        crowns: &[TreeResource {
            id: "minecraft:oak_leaves",
            properties: &[
                ("distance", "7"),
                ("persistent", "false"),
                ("waterlogged", "false"),
            ],
            weight: 1,
        }],
        size: TreeSize::SourceTrunk {
            base: 3,
            random_a: 11,
            random_b: 0,
        },
        shape: TreeShape::Branched,
        decorator_ids: &["minecraft:beehive"],
    },
    TreeProfile {
        id: "minecraft:fancy_oak_leaf_litter",
        source_sha256: "7e54857e43236fd62e6c31291903dbf058237cf90d5c0d4bc74ddd74ef5cdbad",
        stem: TreeResource {
            id: "minecraft:oak_log",
            properties: &[("axis", "y")],
            weight: 1,
        },
        crowns: &[TreeResource {
            id: "minecraft:oak_leaves",
            properties: &[
                ("distance", "7"),
                ("persistent", "false"),
                ("waterlogged", "false"),
            ],
            weight: 1,
        }],
        size: TreeSize::SourceTrunk {
            base: 3,
            random_a: 11,
            random_b: 0,
        },
        shape: TreeShape::Branched,
        decorator_ids: &["minecraft:place_on_ground"],
    },
    TreeProfile {
        id: "minecraft:huge_brown_mushroom",
        source_sha256: "af7dc6b0093dc34974d07a56c9a48e332c9ba72ac3487305037a56e0637bdffc",
        stem: TreeResource {
            id: "minecraft:mushroom_stem",
            properties: &[
                ("down", "false"),
                ("east", "true"),
                ("north", "true"),
                ("south", "true"),
                ("up", "false"),
                ("west", "true"),
            ],
            weight: 1,
        },
        crowns: &[TreeResource {
            id: "minecraft:brown_mushroom_block",
            properties: &[
                ("down", "false"),
                ("east", "true"),
                ("north", "true"),
                ("south", "true"),
                ("up", "true"),
                ("west", "true"),
            ],
            weight: 1,
        }],
        size: TreeSize::AuthoredMushroomHeight {
            minimum: 5,
            maximum: 8,
        },
        shape: TreeShape::BrownMushroom,
        decorator_ids: &[],
    },
    TreeProfile {
        id: "minecraft:huge_red_mushroom",
        source_sha256: "b8fc6f3b9cb98eab5d725a3b898d0b54679e0112677c2a3e4c65f7bae280e3ff",
        stem: TreeResource {
            id: "minecraft:mushroom_stem",
            properties: &[
                ("down", "false"),
                ("east", "true"),
                ("north", "true"),
                ("south", "true"),
                ("up", "false"),
                ("west", "true"),
            ],
            weight: 1,
        },
        crowns: &[TreeResource {
            id: "minecraft:red_mushroom_block",
            properties: &[
                ("down", "false"),
                ("east", "true"),
                ("north", "true"),
                ("south", "true"),
                ("up", "true"),
                ("west", "true"),
            ],
            weight: 1,
        }],
        size: TreeSize::AuthoredMushroomHeight {
            minimum: 5,
            maximum: 8,
        },
        shape: TreeShape::RedMushroom,
        decorator_ids: &[],
    },
    TreeProfile {
        id: "minecraft:jungle_bush",
        source_sha256: "d7619b138edb635b6f865e6aba6939c678ca2603466c13f920add5f91c06639d",
        stem: TreeResource {
            id: "minecraft:jungle_log",
            properties: &[("axis", "y")],
            weight: 1,
        },
        crowns: &[TreeResource {
            id: "minecraft:oak_leaves",
            properties: &[
                ("distance", "7"),
                ("persistent", "false"),
                ("waterlogged", "false"),
            ],
            weight: 1,
        }],
        size: TreeSize::SourceTrunk {
            base: 1,
            random_a: 0,
            random_b: 0,
        },
        shape: TreeShape::Bush,
        decorator_ids: &[],
    },
    TreeProfile {
        id: "minecraft:jungle_tree",
        source_sha256: "e7292293f1e7132a99c874892af8df0022f0017418189ae1f2bf64e19bc599c6",
        stem: TreeResource {
            id: "minecraft:jungle_log",
            properties: &[("axis", "y")],
            weight: 1,
        },
        crowns: &[TreeResource {
            id: "minecraft:jungle_leaves",
            properties: &[
                ("distance", "7"),
                ("persistent", "false"),
                ("waterlogged", "false"),
            ],
            weight: 1,
        }],
        size: TreeSize::SourceTrunk {
            base: 4,
            random_a: 8,
            random_b: 0,
        },
        shape: TreeShape::Rounded,
        decorator_ids: &[
            "minecraft:cocoa",
            "minecraft:leave_vine",
            "minecraft:trunk_vine",
        ],
    },
    TreeProfile {
        id: "minecraft:mangrove",
        source_sha256: "c9e51fc7184b248a565593a6d06f9faf6d7228c5e90fc02d7e31c711f45849fb",
        stem: TreeResource {
            id: "minecraft:mangrove_log",
            properties: &[("axis", "y")],
            weight: 1,
        },
        crowns: &[TreeResource {
            id: "minecraft:mangrove_leaves",
            properties: &[
                ("distance", "7"),
                ("persistent", "false"),
                ("waterlogged", "false"),
            ],
            weight: 1,
        }],
        size: TreeSize::SourceTrunk {
            base: 2,
            random_a: 1,
            random_b: 4,
        },
        shape: TreeShape::Mangrove,
        decorator_ids: &[
            "minecraft:attached_to_leaves",
            "minecraft:beehive",
            "minecraft:leave_vine",
        ],
    },
    TreeProfile {
        id: "minecraft:mega_jungle_tree",
        source_sha256: "02971bee6f3cc2aa454d2352349d2fdea84f8bdd4b73efb8912965ca5c230cb9",
        stem: TreeResource {
            id: "minecraft:jungle_log",
            properties: &[("axis", "y")],
            weight: 1,
        },
        crowns: &[TreeResource {
            id: "minecraft:jungle_leaves",
            properties: &[
                ("distance", "7"),
                ("persistent", "false"),
                ("waterlogged", "false"),
            ],
            weight: 1,
        }],
        size: TreeSize::SourceTrunk {
            base: 10,
            random_a: 2,
            random_b: 19,
        },
        shape: TreeShape::GiantJungle,
        decorator_ids: &["minecraft:leave_vine", "minecraft:trunk_vine"],
    },
    TreeProfile {
        id: "minecraft:mega_pine",
        source_sha256: "ffb58a88426c62142a7f34a72cbdc80907ddddb3dab04f755f0fc70635dc11ab",
        stem: TreeResource {
            id: "minecraft:spruce_log",
            properties: &[("axis", "y")],
            weight: 1,
        },
        crowns: &[TreeResource {
            id: "minecraft:spruce_leaves",
            properties: &[
                ("distance", "7"),
                ("persistent", "false"),
                ("waterlogged", "false"),
            ],
            weight: 1,
        }],
        size: TreeSize::SourceTrunk {
            base: 13,
            random_a: 2,
            random_b: 14,
        },
        shape: TreeShape::GiantPine,
        decorator_ids: &["minecraft:alter_ground"],
    },
    TreeProfile {
        id: "minecraft:mega_spruce",
        source_sha256: "82fc0f06ce91659249a86771c340f3dbfaa79cd0087a375b0a3c46ff6fa1f62e",
        stem: TreeResource {
            id: "minecraft:spruce_log",
            properties: &[("axis", "y")],
            weight: 1,
        },
        crowns: &[TreeResource {
            id: "minecraft:spruce_leaves",
            properties: &[
                ("distance", "7"),
                ("persistent", "false"),
                ("waterlogged", "false"),
            ],
            weight: 1,
        }],
        size: TreeSize::SourceTrunk {
            base: 13,
            random_a: 2,
            random_b: 14,
        },
        shape: TreeShape::GiantSpruce,
        decorator_ids: &["minecraft:alter_ground"],
    },
    TreeProfile {
        id: "minecraft:oak",
        source_sha256: "4bb18fb2039dcb2ed2ef028b874431cf5594deb1e440737b77947b005bf2e352",
        stem: TreeResource {
            id: "minecraft:oak_log",
            properties: &[("axis", "y")],
            weight: 1,
        },
        crowns: &[TreeResource {
            id: "minecraft:oak_leaves",
            properties: &[
                ("distance", "7"),
                ("persistent", "false"),
                ("waterlogged", "false"),
            ],
            weight: 1,
        }],
        size: TreeSize::SourceTrunk {
            base: 4,
            random_a: 2,
            random_b: 0,
        },
        shape: TreeShape::Rounded,
        decorator_ids: &[],
    },
    TreeProfile {
        id: "minecraft:oak_bees_0002_leaf_litter",
        source_sha256: "4fcad251fa992b4dcb97110b54f53d3c80c965f5e0ec4200b9d0398694547140",
        stem: TreeResource {
            id: "minecraft:oak_log",
            properties: &[("axis", "y")],
            weight: 1,
        },
        crowns: &[TreeResource {
            id: "minecraft:oak_leaves",
            properties: &[
                ("distance", "7"),
                ("persistent", "false"),
                ("waterlogged", "false"),
            ],
            weight: 1,
        }],
        size: TreeSize::SourceTrunk {
            base: 4,
            random_a: 2,
            random_b: 0,
        },
        shape: TreeShape::Rounded,
        decorator_ids: &["minecraft:beehive", "minecraft:place_on_ground"],
    },
    TreeProfile {
        id: "minecraft:oak_bees_002",
        source_sha256: "e9ed71dbb243c8e3a2ac7c9e7e259f239b376d9da028bff4c1ee67bd66d0b411",
        stem: TreeResource {
            id: "minecraft:oak_log",
            properties: &[("axis", "y")],
            weight: 1,
        },
        crowns: &[TreeResource {
            id: "minecraft:oak_leaves",
            properties: &[
                ("distance", "7"),
                ("persistent", "false"),
                ("waterlogged", "false"),
            ],
            weight: 1,
        }],
        size: TreeSize::SourceTrunk {
            base: 4,
            random_a: 2,
            random_b: 0,
        },
        shape: TreeShape::Rounded,
        decorator_ids: &["minecraft:beehive"],
    },
    TreeProfile {
        id: "minecraft:oak_bees_005",
        source_sha256: "b38b0fc8a8f2527fbd493efebeaf0d02ca441efe0a5772b6756854338a6683e7",
        stem: TreeResource {
            id: "minecraft:oak_log",
            properties: &[("axis", "y")],
            weight: 1,
        },
        crowns: &[TreeResource {
            id: "minecraft:oak_leaves",
            properties: &[
                ("distance", "7"),
                ("persistent", "false"),
                ("waterlogged", "false"),
            ],
            weight: 1,
        }],
        size: TreeSize::SourceTrunk {
            base: 4,
            random_a: 2,
            random_b: 0,
        },
        shape: TreeShape::Rounded,
        decorator_ids: &["minecraft:beehive"],
    },
    TreeProfile {
        id: "minecraft:oak_leaf_litter",
        source_sha256: "d9ec707f7218532d42052348eba094ab3e32d3473f101ed4ddd189e88eaaa66f",
        stem: TreeResource {
            id: "minecraft:oak_log",
            properties: &[("axis", "y")],
            weight: 1,
        },
        crowns: &[TreeResource {
            id: "minecraft:oak_leaves",
            properties: &[
                ("distance", "7"),
                ("persistent", "false"),
                ("waterlogged", "false"),
            ],
            weight: 1,
        }],
        size: TreeSize::SourceTrunk {
            base: 4,
            random_a: 2,
            random_b: 0,
        },
        shape: TreeShape::Rounded,
        decorator_ids: &["minecraft:place_on_ground"],
    },
    TreeProfile {
        id: "minecraft:orange_poplar_leaf_litter",
        source_sha256: "4fbc75816e95884504b8de16cd883019225c91b7d12b70b0709f88a104ef9049",
        stem: TreeResource {
            id: "minecraft:poplar_log",
            properties: &[("axis", "y")],
            weight: 1,
        },
        crowns: &[TreeResource {
            id: "minecraft:orange_poplar_leaves",
            properties: &[
                ("distance", "7"),
                ("persistent", "false"),
                ("waterlogged", "false"),
            ],
            weight: 1,
        }],
        size: TreeSize::SourceTrunk {
            base: 7,
            random_a: 4,
            random_b: 0,
        },
        shape: TreeShape::Poplar,
        decorator_ids: &["minecraft:place_on_ground", "minecraft:shelf_mushroom"],
    },
    TreeProfile {
        id: "minecraft:pale_oak",
        source_sha256: "c0a1759f8e5578ee2a15f5721343edbc270de1a3c383eaebeabb271ae7dc0a87",
        stem: TreeResource {
            id: "minecraft:pale_oak_log",
            properties: &[("axis", "y")],
            weight: 1,
        },
        crowns: &[TreeResource {
            id: "minecraft:pale_oak_leaves",
            properties: &[
                ("distance", "7"),
                ("persistent", "false"),
                ("waterlogged", "false"),
            ],
            weight: 1,
        }],
        size: TreeSize::SourceTrunk {
            base: 6,
            random_a: 2,
            random_b: 1,
        },
        shape: TreeShape::Dense,
        decorator_ids: &["minecraft:pale_moss"],
    },
    TreeProfile {
        id: "minecraft:pale_oak_creaking",
        source_sha256: "0a8094ae5131c3d6a3c1be83c7676fb0615d42fd71862ae753471dbdb6169295",
        stem: TreeResource {
            id: "minecraft:pale_oak_log",
            properties: &[("axis", "y")],
            weight: 1,
        },
        crowns: &[TreeResource {
            id: "minecraft:pale_oak_leaves",
            properties: &[
                ("distance", "7"),
                ("persistent", "false"),
                ("waterlogged", "false"),
            ],
            weight: 1,
        }],
        size: TreeSize::SourceTrunk {
            base: 6,
            random_a: 2,
            random_b: 1,
        },
        shape: TreeShape::Dense,
        decorator_ids: &["minecraft:creaking_heart", "minecraft:pale_moss"],
    },
    TreeProfile {
        id: "minecraft:pine",
        source_sha256: "75ad9cf6d4bd751859e2540218c8254a7bcedf2dae060d6d7e7d67c09e90b832",
        stem: TreeResource {
            id: "minecraft:spruce_log",
            properties: &[("axis", "y")],
            weight: 1,
        },
        crowns: &[TreeResource {
            id: "minecraft:spruce_leaves",
            properties: &[
                ("distance", "7"),
                ("persistent", "false"),
                ("waterlogged", "false"),
            ],
            weight: 1,
        }],
        size: TreeSize::SourceTrunk {
            base: 6,
            random_a: 4,
            random_b: 0,
        },
        shape: TreeShape::Pine,
        decorator_ids: &[],
    },
    TreeProfile {
        id: "minecraft:red_poplar_leaf_litter",
        source_sha256: "4fff47f1dd119871c2dd2ed8160538f5c8d5703366f10b883636968df384c23a",
        stem: TreeResource {
            id: "minecraft:poplar_log",
            properties: &[("axis", "y")],
            weight: 1,
        },
        crowns: &[TreeResource {
            id: "minecraft:red_poplar_leaves",
            properties: &[
                ("distance", "7"),
                ("persistent", "false"),
                ("waterlogged", "false"),
            ],
            weight: 1,
        }],
        size: TreeSize::SourceTrunk {
            base: 7,
            random_a: 4,
            random_b: 0,
        },
        shape: TreeShape::Poplar,
        decorator_ids: &["minecraft:place_on_ground", "minecraft:shelf_mushroom"],
    },
    TreeProfile {
        id: "minecraft:spruce",
        source_sha256: "ed8091a4dee97cdfd8bcc55132337b8c5a84b5d4b73dd39646b35bdec1c7493a",
        stem: TreeResource {
            id: "minecraft:spruce_log",
            properties: &[("axis", "y")],
            weight: 1,
        },
        crowns: &[TreeResource {
            id: "minecraft:spruce_leaves",
            properties: &[
                ("distance", "7"),
                ("persistent", "false"),
                ("waterlogged", "false"),
            ],
            weight: 1,
        }],
        size: TreeSize::SourceTrunk {
            base: 5,
            random_a: 2,
            random_b: 1,
        },
        shape: TreeShape::Spruce,
        decorator_ids: &[],
    },
    TreeProfile {
        id: "minecraft:super_birch_bees",
        source_sha256: "3db67190dab758c1827333e86884eedfc19992c63dd275d6ae80c5bf0611a8e5",
        stem: TreeResource {
            id: "minecraft:birch_log",
            properties: &[("axis", "y")],
            weight: 1,
        },
        crowns: &[TreeResource {
            id: "minecraft:birch_leaves",
            properties: &[
                ("distance", "7"),
                ("persistent", "false"),
                ("waterlogged", "false"),
            ],
            weight: 1,
        }],
        size: TreeSize::SourceTrunk {
            base: 5,
            random_a: 2,
            random_b: 6,
        },
        shape: TreeShape::Rounded,
        decorator_ids: &["minecraft:beehive"],
    },
    TreeProfile {
        id: "minecraft:super_birch_bees_0002",
        source_sha256: "cb7d81d4bb2e104a5828034d161f50a7af50feede644e0534cd3fae8fe7a967c",
        stem: TreeResource {
            id: "minecraft:birch_log",
            properties: &[("axis", "y")],
            weight: 1,
        },
        crowns: &[TreeResource {
            id: "minecraft:birch_leaves",
            properties: &[
                ("distance", "7"),
                ("persistent", "false"),
                ("waterlogged", "false"),
            ],
            weight: 1,
        }],
        size: TreeSize::SourceTrunk {
            base: 5,
            random_a: 2,
            random_b: 6,
        },
        shape: TreeShape::Rounded,
        decorator_ids: &["minecraft:beehive"],
    },
    TreeProfile {
        id: "minecraft:swamp_oak",
        source_sha256: "a8246dca14243466c360b09880631177953902118882a75d02ed32c9cf1c6551",
        stem: TreeResource {
            id: "minecraft:oak_log",
            properties: &[("axis", "y")],
            weight: 1,
        },
        crowns: &[TreeResource {
            id: "minecraft:oak_leaves",
            properties: &[
                ("distance", "7"),
                ("persistent", "false"),
                ("waterlogged", "false"),
            ],
            weight: 1,
        }],
        size: TreeSize::SourceTrunk {
            base: 5,
            random_a: 3,
            random_b: 0,
        },
        shape: TreeShape::Rounded,
        decorator_ids: &["minecraft:leave_vine"],
    },
    TreeProfile {
        id: "minecraft:tall_mangrove",
        source_sha256: "55d373c5c297d57270a5b94b35c2466480647563fc3e63f5519d3e18dd73a0d0",
        stem: TreeResource {
            id: "minecraft:mangrove_log",
            properties: &[("axis", "y")],
            weight: 1,
        },
        crowns: &[TreeResource {
            id: "minecraft:mangrove_leaves",
            properties: &[
                ("distance", "7"),
                ("persistent", "false"),
                ("waterlogged", "false"),
            ],
            weight: 1,
        }],
        size: TreeSize::SourceTrunk {
            base: 4,
            random_a: 1,
            random_b: 9,
        },
        shape: TreeShape::Mangrove,
        decorator_ids: &[
            "minecraft:attached_to_leaves",
            "minecraft:beehive",
            "minecraft:leave_vine",
        ],
    },
    TreeProfile {
        id: "minecraft:yellow_poplar_leaf_litter",
        source_sha256: "233e363e36d7e15f5ad6935692e4fa4397c12887f546eee4116e2af0632e3b5c",
        stem: TreeResource {
            id: "minecraft:poplar_log",
            properties: &[("axis", "y")],
            weight: 1,
        },
        crowns: &[TreeResource {
            id: "minecraft:yellow_poplar_leaves",
            properties: &[
                ("distance", "7"),
                ("persistent", "false"),
                ("waterlogged", "false"),
            ],
            weight: 1,
        }],
        size: TreeSize::SourceTrunk {
            base: 7,
            random_a: 4,
            random_b: 0,
        },
        shape: TreeShape::Poplar,
        decorator_ids: &["minecraft:place_on_ground", "minecraft:shelf_mushroom"],
    },
    TreeProfile {
        id: "minecraft:azalea_tree",
        source_sha256: "57804c00b56198167b2b38b4d31f68fdfcdbacd0f43bdbc4c9ad49963dd7ba6e",
        stem: TreeResource {
            id: "minecraft:oak_log",
            properties: &[("axis", "y")],
            weight: 1,
        },
        crowns: &[
            TreeResource {
                id: "minecraft:azalea_leaves",
                properties: &[],
                weight: 3,
            },
            TreeResource {
                id: "minecraft:flowering_azalea_leaves",
                properties: &[],
                weight: 1,
            },
        ],
        size: TreeSize::SourceTrunk {
            base: 4,
            random_a: 2,
            random_b: 0,
        },
        shape: TreeShape::Azalea,
        decorator_ids: &[],
    },
];
