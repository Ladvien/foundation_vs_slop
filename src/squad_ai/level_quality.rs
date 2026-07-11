//! The **level-quality objective**: a static, GPU-free structural analysis of a generated level, and
//! the fitness the offline level search maximises.
//!
//! Unlike the behaviour search's *witnessed learnable-surprise* (which needs a full rollout), a level is
//! scored by measuring the artefacts generation already produced — the walkability mask, the placed
//! furniture, and the CPU mycelia habitat mask — against playability/quality heuristics (expressive-range
//! analysis: Smith & Whitehead, "Analyzing the Expressive Range of a Level Generator", PCG 2010). No AI,
//! no physics, no GPU: the whole score is flood-fills and histograms over the generated grid, so it is
//! cheap and deterministic.
//!
//! Two layers, mirroring `surprise.rs`:
//!   1. a hard **minimal criterion** ([`LevelMetrics::passes_criterion`]) — a level that is disconnected,
//!      roomless, or degenerately open/solid is rejected outright (fitness `None`);
//!   2. a scalar **fitness** ([`LevelMetrics::score`]) — a weighted blend of band/target terms over
//!      connectivity, room richness, size hierarchy, furniture balance, and mushroom distribution.
//! The **descriptor axes** ([`LevelMetrics::descriptor_axes`]) illuminate the MAP-Elites archive along
//! two designer dimensions: *openness* (floor fraction) × *infestation* (mould coverage).

use std::collections::VecDeque;

use bevy::math::{IVec2, Vec3};

use crate::dungeon::Dungeon;

/// The measured properties of one generated level. All fractions are in `[0,1]`.
#[derive(Clone, Copy, Debug)]
pub struct LevelMetrics {
    /// Walkable floor cells as a fraction of the whole grid — the "openness" descriptor axis.
    pub floor_fraction: f32,
    /// Habitat (mould) cells as a fraction of floor — the "infestation" descriptor axis.
    pub infestation: f32,
    /// Fraction of floor reachable from the player spawn (a hard connectivity gate).
    pub connected_fraction: f32,
    /// Number of rooms (regions) the generator produced.
    pub room_count: usize,
    /// Coefficient of variation of room floor areas — a size *hierarchy* signal (tiny bathroom beside a
    /// sprawling hall reads as `≈0.7`; every room identical reads as `0`).
    pub room_size_cv: f32,
    /// Total placed furniture pieces.
    pub furniture_count: usize,
    /// Furniture pieces per room.
    pub furniture_per_room: f32,
    /// Of the infested cells, the fraction that lie in rooms (not corridors) — the shipped design rule is
    /// "mould is a room phenomenon" (~80% in rooms), so a level that infests mostly corridors scores low.
    pub mushroom_room_fraction: f32,
}

/// `1.0` inside `[lo, hi]`, decaying linearly to `0` over a margin of half the band width beyond each
/// edge. The workhorse "prefer this range" term.
fn band(v: f32, lo: f32, hi: f32) -> f32 {
    if v >= lo && v <= hi {
        return 1.0;
    }
    let margin = ((hi - lo) * 0.5).max(1e-3);
    let d = if v < lo { lo - v } else { v - hi };
    (1.0 - d / margin).max(0.0)
}

/// `v / target` capped at `1.0` — "more is better up to a target, then plateau".
fn reward_toward(v: f32, target: f32) -> f32 {
    (v / target.max(1e-3)).clamp(0.0, 1.0)
}

impl LevelMetrics {
    /// The two MAP-Elites descriptor axes: openness (floor fraction) × infestation (mould coverage),
    /// each already in `[0,1]`.
    pub fn descriptor_axes(&self) -> (f32, f32) {
        (self.floor_fraction, self.infestation)
    }

    /// The hard admission gate. A level that fails is not scored at all (fitness `None`): every floor cell
    /// must be reachable from the spawn (no orphaned pockets), there must be at least two rooms, and the
    /// floor fraction must be neither near-solid nor near-empty. One path, no degraded fallback.
    pub fn passes_criterion(&self) -> bool {
        self.connected_fraction >= 0.999
            && self.room_count >= 2
            && (0.05..=0.95).contains(&self.floor_fraction)
    }

    /// The scalar level-quality fitness in `[0,1]`, or `None` if the minimal criterion fails. A weighted
    /// blend of band/target terms — "good level" is explicit and tunable here (the design-taste seam).
    pub fn score(&self) -> Option<f32> {
        if !self.passes_criterion() {
            return None;
        }
        // Each term is a quality heuristic in [0,1]; weights sum to 1.0.
        let terms = [
            // Connectivity margin (gated ≥0.999, but keep it as a term so a bare-pass isn't free).
            (0.20, self.connected_fraction),
            // Room richness: a level wants several rooms, not one big hall or a maze of dozens.
            (0.15, band(self.room_count as f32, 6.0, 18.0)),
            // Size hierarchy: varied room sizes read as designed space (Merrell 2011 residential mix).
            (0.15, band(self.room_size_cv, 0.3, 1.2)),
            // Furniture balance: rooms furnished but not crowded.
            (0.15, band(self.furniture_per_room, 1.5, 5.0)),
            // Openness balance: some walls, some space — neither claustrophobic nor a void.
            (0.15, band(self.floor_fraction, 0.25, 0.65)),
            // Mushroom amount: present but not flooding the whole floor.
            (0.10, band(self.infestation, 0.05, 0.35)),
            // Mushroom placement: mould concentrated in rooms, not corridors (the shipped design rule).
            (0.10, reward_toward(self.mushroom_room_fraction, 0.6)),
        ];
        Some(terms.iter().map(|(w, v)| w * v).sum())
    }
}

/// Measure a generated level. `furniture` is `(region, world position)` per placed piece (from
/// `placement::furnish::furnish_all`); `habitat` is the CPU mould mask (from `mycelia::habitat::build`),
/// one `u8` per grid cell in row-major order, a cell being infested where the byte is non-zero.
pub fn measure(dungeon: &Dungeon, furniture: &[(u32, Vec3)], habitat: &[u8]) -> LevelMetrics {
    let (w, h) = (dungeon.width, dungeon.height);
    let total = (w * h) as f32;

    let floor: Vec<IVec2> = dungeon.floor_cells().collect();
    let floor_count = floor.len();
    let floor_fraction = floor_count as f32 / total.max(1.0);

    // Connectivity: BFS from the spawn over the 4-connected walkable mask.
    let reached = reachable_floor(dungeon);
    let connected_fraction = if floor_count == 0 {
        0.0
    } else {
        reached as f32 / floor_count as f32
    };

    // Room areas → size hierarchy (coefficient of variation).
    let areas: Vec<f32> = dungeon
        .regions
        .iter()
        .map(|r| {
            let mut n = 0u32;
            for cy in r.rect.min[1]..r.rect.max[1] {
                for cx in r.rect.min[0]..r.rect.max[0] {
                    if dungeon.is_floor(IVec2::new(cx, cy)) {
                        n += 1;
                    }
                }
            }
            n as f32
        })
        .collect();
    let room_count = areas.len();
    let room_size_cv = coefficient_of_variation(&areas);

    let furniture_count = furniture.len();
    let furniture_per_room = if room_count == 0 {
        0.0
    } else {
        furniture_count as f32 / room_count as f32
    };

    // Mushroom coverage + room-vs-corridor split from the habitat mask.
    let mut infested = 0usize;
    let mut infested_in_rooms = 0usize;
    if habitat.len() == w * h {
        for (i, &v) in habitat.iter().enumerate() {
            if v == 0 {
                continue;
            }
            infested += 1;
            let c = IVec2::new((i % w) as i32, (i / w) as i32);
            if dungeon.is_floor(c) && !dungeon.is_corridor(c) {
                infested_in_rooms += 1;
            }
        }
    }
    let infestation = if floor_count == 0 {
        0.0
    } else {
        infested as f32 / floor_count as f32
    };
    let mushroom_room_fraction = if infested == 0 {
        0.0
    } else {
        infested_in_rooms as f32 / infested as f32
    };

    LevelMetrics {
        floor_fraction,
        infestation,
        connected_fraction,
        room_count,
        room_size_cv,
        furniture_count,
        furniture_per_room,
        mushroom_room_fraction,
    }
}

/// Count floor cells reachable from the spawn via 4-connected flood fill over the walkability mask —
/// the connectivity oracle. A generator that strands a pocket of floor behind solid rock scores a
/// `connected_fraction < 1` and is rejected by the minimal criterion.
fn reachable_floor(dungeon: &Dungeon) -> usize {
    let (w, h) = (dungeon.width, dungeon.height);
    let idx = |c: IVec2| (c.y as usize) * w + (c.x as usize);
    let mut seen = vec![false; w * h];
    let start = dungeon.spawn;
    if !dungeon.is_floor(start) {
        return 0;
    }
    let mut q = VecDeque::new();
    seen[idx(start)] = true;
    q.push_back(start);
    let mut count = 0usize;
    while let Some(c) = q.pop_front() {
        count += 1;
        for d in [IVec2::new(1, 0), IVec2::new(-1, 0), IVec2::new(0, 1), IVec2::new(0, -1)] {
            let n = c + d;
            if n.x < 0 || n.y < 0 || n.x as usize >= w || n.y as usize >= h {
                continue;
            }
            let ni = idx(n);
            if !seen[ni] && dungeon.is_floor(n) {
                seen[ni] = true;
                q.push_back(n);
            }
        }
    }
    count
}

/// Coefficient of variation (std / mean) of a set of positive values — `0` when all equal, growing as
/// the spread widens. Empty or all-zero input yields `0`.
fn coefficient_of_variation(xs: &[f32]) -> f32 {
    if xs.is_empty() {
        return 0.0;
    }
    let n = xs.len() as f32;
    let mean = xs.iter().sum::<f32>() / n;
    if mean <= 0.0 {
        return 0.0;
    }
    let var = xs.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / n;
    var.sqrt() / mean
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a `w×h` dungeon whose floor is a single rectangle, with an optional second isolated pocket.
    fn dungeon_with(w: usize, h: usize, rooms: &[(i32, i32, i32, i32)]) -> Dungeon {
        let mut mask = vec![false; w * h];
        for &(x0, y0, x1, y1) in rooms {
            for y in y0..y1 {
                for x in x0..x1 {
                    mask[(y as usize) * w + (x as usize)] = true;
                }
            }
        }
        Dungeon::from_walkable(w, h, mask)
    }

    #[test]
    fn a_disconnected_level_fails_the_criterion() {
        // Two separate floor rectangles with rock between them: the spawn's component can't reach the
        // other, so connected_fraction < 1 and the minimal criterion rejects the level.
        let mut d = dungeon_with(20, 20, &[(1, 1, 6, 6), (12, 12, 17, 17)]);
        d.spawn = IVec2::new(3, 3);
        let m = measure(&d, &[], &[]);
        assert!(m.connected_fraction < 0.99, "two pockets must not be fully reachable");
        assert!(!m.passes_criterion(), "a disconnected level must be rejected");
        assert!(m.score().is_none());
    }

    #[test]
    fn a_single_connected_room_is_fully_reachable() {
        let mut d = dungeon_with(20, 20, &[(2, 2, 12, 10)]);
        d.spawn = IVec2::new(3, 3);
        let m = measure(&d, &[], &[]);
        assert!((m.connected_fraction - 1.0).abs() < 1e-6, "one room is fully reachable");
        // Floor fraction = 80 cells / 400 = 0.2 → within the [0.05,0.95] open/solid gate.
        assert!(m.floor_fraction > 0.05 && m.floor_fraction < 0.95);
    }

    #[test]
    fn infestation_and_room_split_read_from_the_habitat_mask() {
        // One 10×8 room; mark 16 of its cells infested → infestation = 16/80 = 0.2, all in the room.
        let d = dungeon_with(20, 20, &[(2, 2, 12, 10)]);
        let (w, h) = (d.width, d.height);
        let mut mask = vec![0u8; w * h];
        let mut set = 0;
        'outer: for y in 2..10 {
            for x in 2..12 {
                if set >= 16 {
                    break 'outer;
                }
                mask[y * w + x] = 255;
                set += 1;
            }
        }
        let m = measure(&d, &[], &mask);
        assert!((m.infestation - 0.2).abs() < 1e-6, "16/80 floor infested");
        assert!((m.mushroom_room_fraction - 1.0).abs() < 1e-6, "all infestation is in the room");
    }

    #[test]
    fn band_and_reward_helpers_behave() {
        assert_eq!(band(0.4, 0.25, 0.65), 1.0);
        assert!(band(0.05, 0.25, 0.65) < 1.0);
        assert_eq!(band(1.0, 0.25, 0.65), 0.0);
        assert_eq!(reward_toward(0.6, 0.6), 1.0);
        assert!((reward_toward(0.3, 0.6) - 0.5).abs() < 1e-6);
    }
}
