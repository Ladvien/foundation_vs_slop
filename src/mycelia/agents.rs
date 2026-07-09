//! The Physarum agent population — the "mind" of the mold. Each agent is a point walker on the trail
//! field that senses trail density ahead-left/ahead-right, steers toward the stronger scent, steps, and
//! deposits a fresh scent mark. Thousands of these following Jones' three-sensor rule self-organize into
//! the branching, foraging transport network that reads as *intelligence*.
//!
//! Ref: Jones (2010), "Characteristics of pattern formation and evolution in approximations of Physarum
//! transport networks," *Artificial Life* (arXiv 1503.06579) — the sense→rotate→move→deposit→diffuse→decay
//! loop every real-time GPU Physarum uses (Jenson, Lague).
//!
//! # The floor invariant
//! Agents are seeded **only on walkable floor**, and `agent_step` in `mycelia_sim.wgsl` hard-blocks any
//! move into rock. Together these establish and preserve the invariant *"an agent stands on walkable
//! floor"* — by induction, not by a runtime guard. That is what lets the deposit path drop its walkability
//! check, and it is why seeding must consult the dungeon rather than scattering uniformly: an agent seeded
//! inside rock could never escape a hard block, and would spin against it forever.
//!
//! # GPU layout
//! Seeded on the CPU (deterministically, via [`crate::rng::seeded`]) then handed to the GPU as a
//! `ShaderBuffer`. We encode each agent as four `u32`s — the raw bits of `pos.x`, `pos.y`, `heading`, and a
//! pad word — so `ShaderBuffer::from(Vec<u32>)` lays them out as a tightly-packed `array<u32>` whose bytes
//! are exactly a std430 `array<Agent>` (`Agent { pos: vec2<f32>, heading: f32, _pad: f32 }`, stride 16).
//! GPU float drift *after* seeding is accepted (the whole layer is cosmetic; see `mod.rs` firewall notes).

use rand::RngExt;

use crate::rng;

/// Fixed seed for the initial scatter. The mold's opening arrangement is identical every run; divergence
/// afterward is GPU-side and cosmetic.
const AGENT_SEED: u64 = 0x_C0FFEE_5EED_0FED;

/// Seed `count` agents scattered uniformly across the **walkable** cells of a `grid`×`grid` dungeon,
/// expressed in field-texel coordinates of a `field_size`×`field_size` field, with random headings.
/// Encoded as the flat `u32` bit-buffer described in the module docs (4 words per agent).
///
/// `walkable` is the dungeon's floor mask in row-major `grid`×`grid` order.
///
/// Fails loudly if the mask is the wrong length, or if the dungeon has no floor at all — a level the mold
/// cannot grow in is a generation bug, not a condition to degrade around.
pub fn seed_agents(
    field_size: u32,
    count: u32,
    walkable: &[bool],
    grid: u32,
) -> Result<Vec<u32>, String> {
    let expected = (grid as usize) * (grid as usize);
    if walkable.len() != expected {
        return Err(format!(
            "mycelia: walkable mask is {} cells, expected {grid}x{grid} = {expected}",
            walkable.len()
        ));
    }

    // Index the floor once, then sample it uniformly. Rejection-sampling the whole grid instead would spin
    // arbitrarily long on a mostly-rock dungeon; this is O(1) per agent and gives the same distribution
    // (every cell has identical area).
    let floor: Vec<u32> = (0..expected)
        .filter(|&i| walkable[i])
        .map(|i| i as u32)
        .collect();
    if floor.is_empty() {
        return Err("mycelia: dungeon has no walkable floor to seed agents on".to_string());
    }

    // Field texels per dungeon cell. The compute shader maps a field texel back to its cell with
    // `i32(texel / field_res * control_res)`, so `texel = (cell + frac) * texels_per_cell` is its exact
    // inverse and an agent always lands in the cell we chose.
    let texels_per_cell = field_size as f32 / grid as f32;

    let mut rng = rng::seeded(AGENT_SEED);
    let mut data = Vec::with_capacity(count as usize * 4);
    for _ in 0..count {
        let pick = (rng.random::<f32>() * floor.len() as f32) as usize;
        // `random::<f32>()` is [0,1), but clamp the index anyway: a value of exactly 1.0 from rounding
        // would index one past the end.
        let cell = floor[pick.min(floor.len() - 1)];
        let (cx, cy) = (cell % grid, cell / grid);

        let x = (cx as f32 + rng.random::<f32>()) * texels_per_cell;
        let y = (cy as f32 + rng.random::<f32>()) * texels_per_cell;
        let heading = rng.random::<f32>() * std::f32::consts::TAU;

        data.push(x.to_bits());
        data.push(y.to_bits());
        data.push(heading.to_bits());
        data.push(0u32); // _pad — keeps the per-agent stride at 16 bytes (std430 `Agent`).
    }
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Decode `(x, y)` field-texel positions out of the packed agent buffer.
    fn positions(data: &[u32]) -> Vec<(f32, f32)> {
        data.chunks_exact(4).map(|a| (f32::from_bits(a[0]), f32::from_bits(a[1]))).collect()
    }

    /// A 4x4 grid whose only floor is the 2x2 block at cells (1..=2, 1..=2).
    fn island() -> (Vec<bool>, u32) {
        let grid = 4u32;
        let mut w = vec![false; 16];
        for y in 1..=2 {
            for x in 1..=2 {
                w[y * 4 + x] = true;
            }
        }
        (w, grid)
    }

    /// The invariant the GPU's hard block depends on: no agent may ever be seeded inside rock.
    #[test]
    fn every_agent_seeds_on_walkable_floor() {
        let (walkable, grid) = island();
        let field = 64u32;
        let data = seed_agents(field, 500, &walkable, grid).expect("island has floor");

        let texels_per_cell = field as f32 / grid as f32;
        for (x, y) in positions(&data) {
            assert!((0.0..field as f32).contains(&x), "x {x} out of field");
            assert!((0.0..field as f32).contains(&y), "y {y} out of field");
            // Mirror the shader's texel -> cell map exactly.
            let cx = (x / texels_per_cell) as usize;
            let cy = (y / texels_per_cell) as usize;
            assert!(walkable[cy * grid as usize + cx], "agent seeded in rock at cell ({cx},{cy})");
        }
    }

    #[test]
    fn seeding_is_deterministic() {
        let (walkable, grid) = island();
        let a = seed_agents(128, 200, &walkable, grid).expect("floor");
        let b = seed_agents(128, 200, &walkable, grid).expect("floor");
        assert_eq!(a, b, "same seed must produce an identical scatter");
    }

    #[test]
    fn a_dungeon_with_no_floor_fails_loudly() {
        let err = seed_agents(64, 10, &[false; 16], 4).expect_err("no floor must be an error");
        assert!(err.contains("no walkable floor"), "unhelpful error: {err}");
    }

    #[test]
    fn a_mask_of_the_wrong_size_fails_loudly() {
        let err = seed_agents(64, 10, &[true; 15], 4).expect_err("bad mask must be an error");
        assert!(err.contains("expected 4x4"), "unhelpful error: {err}");
    }

    /// Agents must spread over all of the floor, not clump in one cell.
    #[test]
    fn agents_cover_every_floor_cell() {
        let (walkable, grid) = island();
        let field = 64u32;
        let data = seed_agents(field, 2000, &walkable, grid).expect("floor");
        let texels_per_cell = field as f32 / grid as f32;

        let mut hit = std::collections::HashSet::new();
        for (x, y) in positions(&data) {
            hit.insert(((x / texels_per_cell) as usize, (y / texels_per_cell) as usize));
        }
        assert_eq!(hit.len(), 4, "expected all four floor cells occupied, got {hit:?}");
    }
}
