//! Static game data of the self-playing tower defense: tower and monster
//! kinds, their base statistics, the wave table and the per-level tech.
//!
//! The names and numbers are this scene's own. The progression (maps with
//! waves of cycling monster types, a boss every tenth wave, towers that are
//! built, upgraded and unlocked level by level, interest on banked money,
//! sending waves early for a bonus) follows the game that inspired it.

/// Highest upgrade level any tower can ever reach.
pub const MAX_TOWER_LEVEL: u8 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TowerKind {
    /// Cheap all-round bolts.
    Pulse,
    /// Slow long-range armour-piercing shots.
    Needle,
    /// Lobbed splash shells; ground only.
    Nova,
    /// Slows everything it hits.
    Chill,
    /// Lightning that chains between monsters.
    Arc,
    /// A piercing beam through every monster on a line.
    Lancer,
    /// Volleys of homing missiles.
    Hive,
    /// No attack: boosts every tower around it.
    Beacon,
}

impl TowerKind {
    pub const ALL: [Self; 8] = [
        Self::Pulse,
        Self::Needle,
        Self::Chill,
        Self::Nova,
        Self::Arc,
        Self::Lancer,
        Self::Beacon,
        Self::Hive,
    ];

    pub fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|candidate| *candidate == self)
            .unwrap_or(0)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Pulse => "PULSE",
            Self::Needle => "NEEDLE",
            Self::Nova => "NOVA",
            Self::Chill => "CHILL",
            Self::Arc => "ARC",
            Self::Lancer => "LANCER",
            Self::Hive => "HIVE",
            Self::Beacon => "BEACON",
        }
    }

    /// First stage (cleared levels) at which the AI may build this tower.
    pub fn unlock_stage(self) -> u32 {
        match self {
            Self::Pulse | Self::Needle | Self::Chill => 0,
            Self::Nova => 1,
            Self::Arc => 2,
            Self::Lancer | Self::Beacon => 3,
            Self::Hive => 4,
        }
    }

    pub fn stats(self) -> TowerStats {
        match self {
            Self::Pulse => TowerStats {
                cost: 25.0,
                range: 3.3,
                damage: 6.0,
                cooldown: 0.42,
                speed: 16.0,
                splash: 0.0,
                armor_pierce: 0.0,
                hits_air: true,
            },
            Self::Needle => TowerStats {
                cost: 55.0,
                range: 6.6,
                damage: 36.0,
                cooldown: 1.25,
                speed: 40.0,
                splash: 0.0,
                armor_pierce: 0.6,
                hits_air: true,
            },
            Self::Nova => TowerStats {
                cost: 60.0,
                range: 3.2,
                damage: 11.0,
                cooldown: 0.95,
                speed: 9.0,
                splash: 1.5,
                armor_pierce: 0.0,
                hits_air: false,
            },
            Self::Chill => TowerStats {
                cost: 45.0,
                range: 3.5,
                damage: 2.5,
                cooldown: 0.6,
                speed: 13.0,
                splash: 0.0,
                armor_pierce: 0.0,
                hits_air: true,
            },
            Self::Arc => TowerStats {
                cost: 85.0,
                range: 3.7,
                damage: 14.0,
                cooldown: 0.8,
                speed: 0.0,
                splash: 0.0,
                armor_pierce: 0.2,
                hits_air: true,
            },
            Self::Lancer => TowerStats {
                cost: 120.0,
                range: 5.2,
                damage: 46.0,
                cooldown: 1.7,
                speed: 0.0,
                splash: 0.0,
                armor_pierce: 0.35,
                hits_air: true,
            },
            Self::Hive => TowerStats {
                cost: 140.0,
                range: 4.6,
                damage: 10.0,
                cooldown: 1.5,
                speed: 8.0,
                splash: 0.7,
                armor_pierce: 0.0,
                hits_air: true,
            },
            Self::Beacon => TowerStats {
                cost: 80.0,
                range: 2.7,
                damage: 0.0,
                cooldown: 1.0,
                speed: 0.0,
                splash: 0.0,
                armor_pierce: 0.0,
                hits_air: true,
            },
        }
    }

    /// Missiles launched per volley.
    pub fn volley(self) -> u32 {
        match self {
            Self::Hive => 3,
            _ => 1,
        }
    }

    /// Monsters an Arc bolt can jump to after the first.
    pub fn chain(self, level: u8) -> u32 {
        match self {
            Self::Arc => 2 + u32::from(level) / 2,
            _ => 0,
        }
    }

    /// Slow applied by a hit: (speed multiplier, seconds).
    pub fn slow(self, level: u8) -> Option<(f32, f32)> {
        match self {
            Self::Chill => Some((
                (0.58 - 0.05 * f32::from(level - 1)).max(0.25),
                1.5 + 0.2 * f32::from(level - 1),
            )),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TowerStats {
    pub cost: f32,
    /// Cells.
    pub range: f32,
    pub damage: f32,
    /// Seconds between shots.
    pub cooldown: f32,
    /// Projectile speed in cells per second; 0 for instant attacks.
    pub speed: f32,
    /// Splash radius in cells.
    pub splash: f32,
    /// Fraction of a monster's armour ignored.
    pub armor_pierce: f32,
    pub hits_air: bool,
}

/// Damage multiplier of upgrade level `level` (1 based).
pub fn level_damage(level: u8) -> f32 {
    1.0 + 0.6 * f32::from(level - 1)
}

pub fn level_range(level: u8) -> f32 {
    1.0 + 0.06 * f32::from(level - 1)
}

pub fn level_cooldown(level: u8) -> f32 {
    (1.0 - 0.055 * f32::from(level - 1)).max(0.5)
}

/// Price to upgrade a tower of `kind` from `level` to `level + 1`.
pub fn upgrade_cost(kind: TowerKind, level: u8) -> f32 {
    (kind.stats().cost * (0.55 + 0.4 * f32::from(level))).round()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MonsterKind {
    /// The ordinary grunt.
    Drone,
    /// Quick and fragile.
    Dart,
    /// Plated: flat damage reduction.
    Shell,
    /// Breaks into two smaller monsters when it dies.
    Splitter,
    /// A crowd of small monsters.
    Swarm,
    /// Flies straight from entrance to exit, ignoring the path.
    Wisp,
    /// A huge slow monster with rotating armour.
    Boss,
    /// What a splitter leaves behind.
    Shard,
}

impl MonsterKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Drone => "DRONES",
            Self::Dart => "FAST DARTS",
            Self::Shell => "ARMOURED SHELLS",
            Self::Splitter => "SPLITTERS",
            Self::Swarm => "SWARM",
            Self::Wisp => "FLYING WISPS",
            Self::Boss => "BOSS",
            Self::Shard => "SHARDS",
        }
    }

    pub fn is_flying(self) -> bool {
        matches!(self, Self::Wisp)
    }

    /// Health before wave and level scaling.
    pub fn base_hp(self) -> f32 {
        match self {
            Self::Drone => 30.0,
            Self::Dart => 17.0,
            Self::Shell => 62.0,
            Self::Splitter => 44.0,
            Self::Swarm => 12.0,
            Self::Wisp => 26.0,
            Self::Boss => 520.0,
            Self::Shard => 18.0,
        }
    }

    /// Cells per second.
    pub fn speed(self) -> f32 {
        match self {
            Self::Drone => 1.6,
            Self::Dart => 3.0,
            Self::Shell => 1.25,
            Self::Splitter => 1.4,
            Self::Swarm => 2.0,
            Self::Wisp => 2.1,
            Self::Boss => 0.85,
            Self::Shard => 2.0,
        }
    }

    /// Flat damage removed from every hit (before piercing).
    pub fn armor(self, wave: u32) -> f32 {
        match self {
            Self::Shell => 2.0 + wave as f32 * 0.35,
            Self::Boss => 3.0 + wave as f32 * 0.3,
            _ => 0.0,
        }
    }

    /// How strongly slows work on this monster, 0..=1.
    pub fn slow_effect(self) -> f32 {
        match self {
            Self::Boss => 0.5,
            _ => 1.0,
        }
    }

    /// Drawn radius in cells.
    pub fn radius(self) -> f32 {
        match self {
            Self::Drone => 0.36,
            Self::Dart => 0.32,
            Self::Shell => 0.42,
            Self::Splitter => 0.42,
            Self::Swarm => 0.22,
            Self::Wisp => 0.34,
            Self::Boss => 0.95,
            Self::Shard => 0.22,
        }
    }

    /// Gold multiplier of a kill.
    pub fn bounty_factor(self) -> f32 {
        match self {
            Self::Drone => 1.0,
            Self::Dart => 1.2,
            Self::Shell => 1.7,
            Self::Splitter => 1.5,
            Self::Swarm => 0.45,
            Self::Wisp => 1.3,
            Self::Boss => 18.0,
            Self::Shard => 0.3,
        }
    }

    /// Lives lost when it leaks.
    pub fn leak_cost(self) -> i32 {
        match self {
            Self::Boss => 5,
            _ => 1,
        }
    }
}

/// One entry of a wave: `count` monsters of `kind`, one every `gap` seconds.
#[derive(Debug, Clone, Copy)]
pub struct WaveGroup {
    pub kind: MonsterKind,
    pub count: u32,
    pub gap: f32,
}

/// What wave `wave` (1 based) sends. The ten-wave cycle mirrors the original
/// rhythm: plain waves, then a crowd, fast, armoured, splitting and flying
/// waves, with a boss closing every cycle.
pub fn wave_groups(wave: u32) -> Vec<WaveGroup> {
    let grow = wave / 4;
    let group = |kind, count: u32, gap| WaveGroup { kind, count, gap };
    match (wave - 1) % 10 {
        0 => vec![group(MonsterKind::Drone, 9 + grow, 0.9)],
        1 => vec![group(MonsterKind::Drone, 11 + grow, 0.8)],
        2 => vec![group(MonsterKind::Swarm, 22 + grow * 3, 0.32)],
        3 => vec![group(MonsterKind::Dart, 10 + grow, 0.55)],
        4 => vec![group(MonsterKind::Shell, 8 + grow, 1.1)],
        5 => vec![group(MonsterKind::Splitter, 8 + grow, 1.0)],
        6 => vec![group(MonsterKind::Wisp, 10 + grow, 0.7)],
        7 => vec![
            group(MonsterKind::Swarm, 18 + grow * 3, 0.3),
            group(MonsterKind::Shell, 4 + grow / 2, 1.2),
        ],
        8 => vec![
            group(MonsterKind::Dart, 12 + grow, 0.45),
            group(MonsterKind::Wisp, 6 + grow / 2, 0.7),
        ],
        _ => vec![
            group(MonsterKind::Boss, 1 + wave / 20, 2.0),
            group(MonsterKind::Drone, 8 + grow, 0.7),
        ],
    }
}

/// Health multiplier of wave `wave` on stage `stage` (cleared levels).
pub fn hp_scale(wave: u32, stage: u32) -> f32 {
    let w = wave as f32 - 1.0;
    let within = 1.0 + 0.26 * w + 0.011 * w * w;
    within * 1.25f32.powi(stage as i32)
}

/// Gold a kill of `kind` pays on wave `wave`.
pub fn bounty(kind: MonsterKind, wave: u32, stage: u32) -> f32 {
    (2.0 + 0.55 * wave as f32 + 0.8 * stage as f32) * kind.bounty_factor()
}

/// Gold paid for clearing a wave without leaks.
pub fn wave_bonus(wave: u32) -> f32 {
    24.0 + 3.0 * wave as f32
}

/// Tower damage multiplier from the tech earned so far.
pub fn tech_damage(stage: u32, defeats: u32) -> f32 {
    1.0 + 0.12 * stage as f32 + 0.06 * defeats as f32
}

/// Highest upgrade level the current tech allows.
pub fn max_level(stage: u32) -> u8 {
    (3 + stage.min(5) as u8).min(MAX_TOWER_LEVEL)
}

pub fn starting_money(stage: u32) -> f32 {
    110.0 + 35.0 * stage as f32
}

pub const STARTING_LIVES: i32 = 20;

/// Monster health multiplier that eases off after each lost attempt, so a
/// level the AI cannot beat eventually becomes winnable.
pub fn retry_relief(defeats: u32) -> f32 {
    0.9f32.powi(defeats as i32).max(0.45)
}
