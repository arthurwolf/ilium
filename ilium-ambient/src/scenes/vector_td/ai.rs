//! The player AI: decides where to build, what to upgrade and when to send
//! the next wave early, by comparing the expected damage each purchase adds
//! per credit.
//!
//! Every tower has a "value": the damage it deals to one monster that walks
//! the whole path past it (damage per second times the seconds the monster
//! spends in range). Where a tower stands decides its coverage, so the choice
//! of cell is a search over `Level::coverage`. Support towers are valued by
//! what they add to their neighbours. The needs of the coming waves (armour,
//! crowds, flyers, bosses) weight the kinds.

use super::model::{self, MonsterKind, TowerKind};
use super::sim::{Game, Phase};

/// Seconds between two AI decisions.
pub const THINK_SECONDS: f32 = 0.45;
/// Typical monster speed used to turn coverage length into exposure time.
const TYPICAL_SPEED: f32 = 1.6;
/// Most purchases per decision.
const ACTIONS_PER_THINK: usize = 5;
/// Fraction of the best option's score an affordable alternative needs for
/// the AI to buy it now instead of saving up.
const SETTLE_RATIO: f32 = 0.7;

/// What the upcoming waves ask of the defence, each 0..=1.
#[derive(Debug, Clone, Copy, Default)]
pub struct Needs {
    pub air: f32,
    pub armor: f32,
    pub crowd: f32,
    pub boss: f32,
    /// Typical health of one monster in the coming waves: shots that deal
    /// more than this waste the excess.
    pub typical_hp: f32,
}

fn needs_of(game: &Game) -> Needs {
    let mut needs = Needs::default();
    let (mut hp_sum, mut count_sum) = (0.0f32, 0.0f32);
    for (offset, weight) in [(1u32, 1.0f32), (2, 0.6), (3, 0.3)] {
        let wave = game.wave + offset;
        if wave > game.rules.waves {
            break;
        }
        let scale = model::hp_scale(wave, game.stage) * game.rules.difficulty;
        for group in model::wave_groups(wave) {
            if group.kind != MonsterKind::Boss {
                let share = weight * group.count as f32;
                hp_sum += share * group.kind.base_hp() * scale;
                count_sum += share;
            }
            let amount = weight * (group.count as f32 / 12.0).min(1.0);
            match group.kind {
                MonsterKind::Wisp => needs.air = needs.air.max(amount),
                MonsterKind::Shell => needs.armor = needs.armor.max(amount),
                MonsterKind::Swarm | MonsterKind::Splitter => {
                    needs.crowd = needs.crowd.max(amount);
                }
                MonsterKind::Boss => {
                    needs.boss = needs.boss.max(weight);
                    needs.armor = needs.armor.max(weight * 0.6);
                }
                _ => {}
            }
        }
    }
    needs.typical_hp = if count_sum > 0.0 {
        hp_sum / count_sum
    } else {
        1000.0
    };
    needs
}

fn kind_weight(kind: TowerKind, needs: &Needs) -> f32 {
    match kind {
        TowerKind::Pulse => 1.0 - 0.35 * needs.armor,
        TowerKind::Needle => 1.0 + 0.5 * needs.armor + 0.3 * needs.boss + 0.2 * needs.air,
        TowerKind::Nova => (1.0 + 0.5 * needs.crowd) * (1.0 - 0.6 * needs.air),
        TowerKind::Chill => 1.0 + 0.3 * needs.boss,
        TowerKind::Arc => 1.0 + 0.45 * needs.crowd,
        TowerKind::Lancer => 1.0 + 0.4 * needs.armor + 0.3 * needs.boss,
        TowerKind::Hive => 1.0 + 0.3 * needs.air,
        TowerKind::Beacon => 1.0,
    }
}

/// Damage per second a tower of `kind` and `level` deals to one target.
fn dps(kind: TowerKind, level: u8, tech: f32, typical_hp: f32) -> f32 {
    let stats = kind.stats();
    if stats.damage <= 0.0 {
        return 0.0;
    }
    // A shot cannot kill more than one monster's health: excess is wasted.
    let shot = (stats.damage * model::level_damage(level) * tech).min(typical_hp * 1.3);
    shot * kind.volley() as f32 / (stats.cooldown * model::level_cooldown(level))
}

/// Damage dealt to one monster that crosses `cover` cells of coverage,
/// including what the tower's special adds.
fn value(kind: TowerKind, level: u8, cover: f32, straight: f32, tech: f32, needs: &Needs) -> f32 {
    let range_gain = model::level_range(level);
    let exposure = cover * range_gain / TYPICAL_SPEED;
    let base = dps(kind, level, tech, needs.typical_hp) * exposure;
    let armor_edge = 1.0 + kind.stats().armor_pierce * needs.armor * 1.4;
    match kind {
        TowerKind::Pulse | TowerKind::Needle => base * armor_edge,
        // Splash hits several monsters of a stream at once.
        TowerKind::Nova => base * (2.0 + needs.crowd),
        TowerKind::Chill => 0.0,
        TowerKind::Arc => base * (1.0 + 0.6 * kind.chain(level) as f32),
        TowerKind::Lancer => base * armor_edge * (1.0 + straight / 3.0),
        TowerKind::Hive => base * 0.9,
        TowerKind::Beacon => 0.0,
    }
}

/// Cover and straight-run of the cell under (or planned for) a tower.
fn cell_cover(game: &Game, kind: TowerKind, cell: (i32, i32)) -> (f32, f32) {
    match game.level.buildable_index(cell) {
        Some(index) => (
            game.level.coverage[kind.index()][index],
            game.level.straight[index],
        ),
        None => (0.0, 0.0),
    }
}

fn tower_value(game: &Game, index: usize, level: u8, needs: &Needs) -> f32 {
    let tower = &game.towers[index];
    let (cover, straight) = cell_cover(game, tower.kind, tower.cell);
    value(tower.kind, level, cover, straight, game.tech, needs) * tower.damage_boost
}

#[derive(Debug, Clone, Copy)]
enum Action {
    Build(TowerKind, (i32, i32)),
    Upgrade(usize),
}

#[derive(Debug, Clone, Copy)]
struct Purchase {
    action: Action,
    cost: f32,
    /// Value the purchase adds, before dividing by its cost.
    gain: f32,
    score: f32,
}

fn build_option(game: &Game, kind: TowerKind, needs: &Needs) -> Option<Purchase> {
    let cost = kind.stats().cost;
    // Every extra tower of one kind is a little less attractive than a
    // first of another kind, so bases stay varied.
    let owned = game
        .towers
        .iter()
        .filter(|tower| tower.kind == kind)
        .count();
    let variety = 1.0 / (1.0 + 0.09 * owned as f32);
    let mut best: Option<((i32, i32), f32)> = None;
    for (index, &cell) in game.level.buildable.iter().enumerate() {
        if game.tower_at(cell).is_some() {
            continue;
        }
        let cover = game.level.coverage[kind.index()][index];
        if cover < 2.0 {
            continue;
        }
        let straight = game.level.straight[index];
        let added = match kind {
            TowerKind::Chill => chill_gain(game, index, 1, needs),
            TowerKind::Beacon => beacon_gain(game, cell, needs),
            _ => value(kind, 1, cover, straight, game.tech, needs),
        };
        // Ties and near-ties break differently for every seed, so two games
        // with different seeds do not build identical bases.
        let jitter = 1.0 + 0.04 * cell_noise(game.seed, cell);
        let score = added * kind_weight(kind, needs) / cost * jitter * variety;
        if best.is_none_or(|(_, current)| score > current) {
            best = Some((cell, score));
        }
    }
    let (cell, score) = best?;
    if score <= 0.0 {
        return None;
    }
    let gain = score * cost;
    Some(Purchase {
        action: Action::Build(kind, cell),
        cost,
        gain,
        score,
    })
}

/// Damage a Chill of `level` at buildable cell `index` adds: every tower
/// that shares path with it deals `1 / factor` as much to slowed monsters
/// over the shared stretch.
fn chill_gain(game: &Game, index: usize, level: u8, needs: &Needs) -> f32 {
    chill_gain_excluding(game, index, level, needs, None)
}

/// As `chill_gain`, ignoring tower `skip` (the Chill being upgraded) when
/// working out which stretch of path is already slowed: slows do not stack.
fn chill_gain_excluding(
    game: &Game,
    index: usize,
    level: u8,
    needs: &Needs,
    skip: Option<usize>,
) -> f32 {
    let factor = TowerKind::Chill
        .slow(level)
        .map_or(1.0, |(factor, _)| factor);
    let chill_mask = &game.level.masks[TowerKind::Chill.index()][index];
    let mut slowed = vec![0u64; chill_mask.len()];
    for (tower_index, tower) in game.towers.iter().enumerate() {
        if tower.kind != TowerKind::Chill || Some(tower_index) == skip {
            continue;
        }
        if let Some(cell_index) = game.level.buildable_index(tower.cell) {
            for (word, other) in slowed
                .iter_mut()
                .zip(&game.level.masks[TowerKind::Chill.index()][cell_index])
            {
                *word |= other;
            }
        }
    }
    let mut gain = 0.0;
    for (tower_index, tower) in game.towers.iter().enumerate() {
        if matches!(tower.kind, TowerKind::Beacon | TowerKind::Chill) {
            continue;
        }
        let Some(cell_index) = game.level.buildable_index(tower.cell) else {
            continue;
        };
        let mask = &game.level.masks[tower.kind.index()][cell_index];
        let shared: u32 = chill_mask
            .iter()
            .zip(mask)
            .zip(&slowed)
            .map(|((a, b), already)| (a & b & !already).count_ones())
            .sum();
        let (cover, _) = cell_cover(game, tower.kind, tower.cell);
        if cover <= 0.0 {
            continue;
        }
        let fraction = (shared as f32 * super::maps::SAMPLE_STEP / cover).min(1.0);
        gain +=
            tower_value(game, tower_index, tower.level, needs) * fraction * (1.0 / factor - 1.0);
    }
    gain + 1.0
}

/// Deterministic noise in -1..1 for a seed and cell.
fn cell_noise(seed: u64, cell: (i32, i32)) -> f32 {
    let mut value = seed
        .wrapping_add((cell.0 as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15))
        .wrapping_add((cell.1 as u64).wrapping_mul(0xbf58_476d_1ce4_e5b9));
    value ^= value >> 30;
    value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^= value >> 27;
    ((value >> 40) as f32 / (1u64 << 24) as f32) * 2.0 - 1.0
}

/// What a Beacon at `cell` would add: a share of its neighbours' value.
fn beacon_gain(game: &Game, cell: (i32, i32), needs: &Needs) -> f32 {
    let range = TowerKind::Beacon.stats().range;
    let mut gain = 0.0;
    for (index, tower) in game.towers.iter().enumerate() {
        if tower.kind == TowerKind::Beacon {
            continue;
        }
        let (dx, dy) = (
            (tower.cell.0 - cell.0) as f32,
            (tower.cell.1 - cell.1) as f32,
        );
        if dx * dx + dy * dy <= range * range {
            gain += 0.27 * tower_value(game, index, tower.level, needs) / tower.damage_boost;
        }
    }
    gain
}

fn upgrade_option(game: &Game, index: usize, needs: &Needs) -> Option<Purchase> {
    let tower = &game.towers[index];
    if tower.level >= game.max_tower_level() {
        return None;
    }
    let cost = model::upgrade_cost(tower.kind, tower.level);
    let (cover, straight) = cell_cover(game, tower.kind, tower.cell);
    let gain = match tower.kind {
        TowerKind::Chill => match game.level.buildable_index(tower.cell) {
            Some(cell_index) => {
                chill_gain_excluding(game, cell_index, tower.level + 1, needs, Some(index))
                    - chill_gain_excluding(game, cell_index, tower.level, needs, Some(index))
            }
            None => 0.0,
        },
        TowerKind::Beacon => {
            let mut sum = 0.0;
            for (other_index, other) in game.towers.iter().enumerate() {
                if other.kind == TowerKind::Beacon {
                    continue;
                }
                let (dx, dy) = (
                    (other.cell.0 - tower.cell.0) as f32,
                    (other.cell.1 - tower.cell.1) as f32,
                );
                if dx * dx + dy * dy <= tower.range() * tower.range() {
                    sum += tower_value(game, other_index, other.level, needs) / other.damage_boost;
                }
            }
            0.06 * sum
        }
        kind => {
            (value(kind, tower.level + 1, cover, straight, game.tech, needs)
                - value(kind, tower.level, cover, straight, game.tech, needs))
                * tower.damage_boost
        }
    };
    Some(Purchase {
        action: Action::Upgrade(index),
        cost,
        gain,
        score: gain * kind_weight(tower.kind, needs) / cost,
    })
}

fn all_options(game: &Game, with_builds: bool) -> Vec<Purchase> {
    let needs = needs_of(game);
    let mut options = Vec::new();
    for kind in TowerKind::ALL {
        if !with_builds || !game.is_unlocked(kind) {
            continue;
        }
        options.extend(build_option(game, kind, &needs));
    }
    for index in 0..game.towers.len() {
        options.extend(upgrade_option(game, index, &needs));
    }
    options
}

fn execute(game: &mut Game, option: &Purchase) -> bool {
    match option.action {
        Action::Build(kind, cell) => game.build(kind, cell),
        Action::Upgrade(index) => game.upgrade(index),
    }
}

/// One decision: buy things while the best affordable option is nearly as
/// good as the best option overall, otherwise save up.
pub fn think(game: &mut Game) {
    for _ in 0..ACTIONS_PER_THINK {
        let capped = game.towers.len() >= tower_cap(game);
        let options = all_options(game, !capped);
        let Some(best) = options
            .iter()
            .copied()
            .max_by(|a, b| a.score.total_cmp(&b.score))
        else {
            if capped {
                replace_weakest(game);
            }
            break;
        };
        let affordable = options
            .iter()
            .copied()
            .filter(|option| option.cost <= game.money)
            .max_by(|a, b| a.score.total_cmp(&b.score));
        let Some(choice) = affordable else {
            if capped {
                replace_weakest(game);
            }
            break;
        };
        // Saving up only pays when the better purchase is nearly in reach.
        let almost_there = best.cost <= game.money * 1.6 + 8.0;
        if choice.score < best.score * SETTLE_RATIO && almost_there {
            break;
        }
        if !execute(game, &choice) {
            break;
        }
    }
    consider_sending_early(game);
}

/// Most towers the AI keeps at once: more levels, more room to manage.
fn tower_cap(game: &Game) -> usize {
    12 + 4 * game.stage.min(7) as usize
}

/// With the board full and everything upgraded, trade the weakest tower for
/// a clearly better one so the base keeps evolving.
fn replace_weakest(game: &mut Game) {
    if game.money < 120.0 {
        return;
    }
    let needs = needs_of(game);
    let Some((weakest, weakest_value)) = (0..game.towers.len())
        .filter(|&index| {
            !matches!(
                game.towers[index].kind,
                TowerKind::Beacon | TowerKind::Chill
            )
        })
        .map(|index| {
            (
                index,
                tower_value(game, index, game.towers[index].level, &needs),
            )
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
    else {
        return;
    };
    let best_gain = TowerKind::ALL
        .iter()
        .filter(|kind| game.is_unlocked(**kind))
        .filter_map(|kind| build_option(game, *kind, &needs))
        .map(|purchase| purchase.gain)
        .fold(0.0f32, f32::max);
    if best_gain > weakest_value * 2.0 {
        game.sell(weakest);
    }
}

/// Damage the defence can deliver over a wave, against the wave's health.
fn defence_ratio(game: &Game) -> f32 {
    let needs = needs_of(game);
    let next_wave = game.wave + 1;
    let total_hp = game.next_wave_hp().max(1.0);
    let span: f32 = model::wave_groups(next_wave)
        .iter()
        .map(|group| group.count as f32 * group.gap)
        .sum();
    let mut capacity = 0.0;
    for (index, tower) in game.towers.iter().enumerate() {
        if matches!(tower.kind, TowerKind::Beacon | TowerKind::Chill) {
            continue;
        }
        let (cover, _) = cell_cover(game, tower.kind, tower.cell);
        let per_pass = tower_value(game, index, tower.level, &needs);
        let exposure = (cover * model::level_range(tower.level) / TYPICAL_SPEED).max(0.1);
        let power = per_pass / exposure;
        capacity += power * (exposure + span * 0.5);
    }
    capacity / total_hp
}

/// Send the next wave early when the field is nearly empty and the defence
/// clearly outclasses it.
fn consider_sending_early(game: &mut Game) {
    if game.phase != Phase::Running || game.wave >= game.rules.waves {
        return;
    }
    if game.lives < 8 || game.since_wave < 5.0 {
        return;
    }
    let next_hp = game.next_wave_hp();
    if game.threat_hp() > next_hp * 0.2 {
        return;
    }
    if defence_ratio(game) >= 2.4 {
        game.send_wave_early();
    }
}
