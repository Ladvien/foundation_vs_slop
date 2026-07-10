//! The Physarum agent population — the "mind" of the mold. Each agent is a point walker on the trail
//! field that senses trail density ahead-left/ahead-right, steers toward the stronger scent, steps, and
//! deposits a fresh scent mark. Thousands of these following Jones' three-sensor rule self-organize into
//! the branching, foraging transport network that reads as *intelligence*.
//!
//! Ref: Jones (2010), "Characteristics of pattern formation and evolution in approximations of Physarum
//! transport networks," *Artificial Life* (arXiv 1503.06579) — the sense→rotate→move→deposit→diffuse→decay
//! loop every real-time GPU Physarum uses (Jenson, Lague).
//!
//! # The habitat invariant
//! Agents are seeded **only inside the habitat mask**, and `agent_step` in `mycelia_sim.wgsl` hard-blocks any
//! move out of it. Together these establish and preserve the invariant *"an agent stands on habitable
//! floor"* — by induction, not by a runtime guard. That is what lets the deposit path drop its walkability
//! check, and it is why seeding must consult the mask rather than scattering uniformly: an agent seeded
//! outside it could never escape a hard block, and would spin against it forever.
//!
//! Two consequences worth stating, because both are load-bearing:
//!
//! 1. **The mask must be static.** The block is keyed on `habitat.rs`'s startup mask, never on the *dynamic*
//!    control texture. If a transient signal — a blood pool, say — could widen the habitat, then every agent
//!    that walked onto the widened ground would be stranded the moment the pool faded, with no habitable
//!    neighbour to step to. Rock works as a hard block precisely because rock never moves.
//! 2. **Confining the agents confines the mold.** `bloom_seed` nucleates biomass under a *vein*, and veins
//!    exist only where agents deposited. So no separate mask is needed on the Gray-Scott field: it follows.
//!
//! Seeding therefore reads the same **`u8`** bytes the shader reads (the static control texture's `G`
//! channel) and thresholds them at the same `agent_hab_min`. Seeding from the pre-quantization `f32` would
//! let a texel just above the threshold round to a byte just below it, and that agent would never move again.
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

/// Seed `count` agents scattered uniformly across the **habitable** texels of a `field_size`×`field_size`
/// field, with random headings. Encoded as the flat `u32` bit-buffer described in the module docs (4 words
/// per agent).
///
/// `habitat` is the quantized habitat mask in row-major field order — byte-for-byte what the shader samples
/// from the static control texture's `G` channel. `hab_min` is `MyceliaConfig::agent_hab_min`, applied here
/// exactly as `agent_step` applies it, so the two agree on every texel.
///
/// Placement is at **field** resolution, so an agent lands sub-tile inside a patch rather than being
/// quantized to a dungeon cell — which matters, because a patch border falls between cells.
///
/// Fails loudly if the mask is the wrong length, or if nothing at all is habitable — a level the mold cannot
/// grow in is a generation bug, not a condition to degrade around.
pub fn seed_agents(
    field_size: u32,
    count: u32,
    habitat: &[u8],
    hab_min: f32,
) -> Result<Vec<u32>, String> {
    let expected = (field_size as usize) * (field_size as usize);
    if habitat.len() != expected {
        return Err(format!(
            "mycelia: habitat mask is {} texels, expected {field_size}x{field_size} = {expected}",
            habitat.len()
        ));
    }

    // Threshold on the byte, not on a reconstructed float: `>= ceil(hab_min * 255)` is exactly the set of
    // bytes whose `u8 -> unorm` value the shader will accept. Deriving the cut once here keeps the CPU and
    // GPU on the same side of it for every texel.
    let cut = (hab_min * 255.0).ceil() as u32;
    let cut = u8::try_from(cut.min(255)).unwrap_or(255);

    // Index the habitable texels once, then sample uniformly. Rejection-sampling the whole field instead
    // would spin arbitrarily long on a sparsely-infested dungeon (the whole point of this feature); this is
    // O(1) per agent and gives the same distribution, since every texel has identical area.
    let habitable: Vec<u32> = (0..expected)
        .filter(|&i| habitat[i] >= cut)
        .map(|i| i as u32)
        .collect();
    if habitable.is_empty() {
        return Err("mycelia: habitat mask has no habitable texel to seed agents on".to_string());
    }

    let mut rng = rng::seeded(AGENT_SEED);
    let mut data = Vec::with_capacity(count as usize * 4);
    for _ in 0..count {
        let pick = (rng.random::<f32>() * habitable.len() as f32) as usize;
        // `random::<f32>()` is [0,1), but clamp the index anyway: a value of exactly 1.0 from rounding
        // would index one past the end.
        let texel = habitable[pick.min(habitable.len() - 1)];
        let (tx, ty) = (texel % field_size, texel / field_size);

        // Jitter within the texel. `agent_step` floors a position to its texel, so `tx + [0,1)` stays inside
        // the texel we chose and the seeding invariant holds exactly.
        let x = tx as f32 + rng.random::<f32>();
        let y = ty as f32 + rng.random::<f32>();
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

    /// `agent_hab_min` as shipped; the byte cut is `ceil(0.02 * 255) = 6`.
    const HAB_MIN: f32 = 0.02;

    /// A 64² habitat mask whose only habitable region is the 16×16 texel block at (16..32, 16..32).
    fn island(field: u32) -> Vec<u8> {
        let mut h = vec![0u8; (field * field) as usize];
        for y in 16..32u32 {
            for x in 16..32u32 {
                h[(y * field + x) as usize] = 255;
            }
        }
        h
    }

    /// The invariant the GPU's hard block depends on: no agent may ever be seeded outside the habitat.
    #[test]
    fn every_agent_seeds_inside_the_habitat() {
        let field = 64u32;
        let habitat = island(field);
        let data = seed_agents(field, 500, &habitat, HAB_MIN).expect("island is habitable");

        for (x, y) in positions(&data) {
            assert!((0.0..field as f32).contains(&x), "x {x} out of field");
            assert!((0.0..field as f32).contains(&y), "y {y} out of field");
            // Mirror `agent_step`'s position -> texel map exactly.
            let (tx, ty) = (x.floor() as u32, y.floor() as u32);
            assert!(
                habitat[(ty * field + tx) as usize] >= 6,
                "agent seeded on barren texel ({tx},{ty})"
            );
        }
    }

    /// A texel whose byte sits just under the cut must never be seeded on — this is the exact rounding
    /// boundary where a `f32`-seeded agent would be handed to a GPU that refuses to let it move.
    #[test]
    fn the_threshold_is_applied_on_the_byte_not_a_float() {
        let field = 8u32;
        let mut habitat = vec![0u8; 64];
        habitat[0] = 5; // just below ceil(0.02 * 255) = 6
        assert!(seed_agents(field, 4, &habitat, HAB_MIN).is_err(), "5 must be barren");
        habitat[0] = 6; // exactly at the cut
        let data = seed_agents(field, 4, &habitat, HAB_MIN).expect("6 must be habitable");
        for (x, y) in positions(&data) {
            assert_eq!((x.floor(), y.floor()), (0.0, 0.0), "only texel 0 is habitable");
        }
    }

    #[test]
    fn seeding_is_deterministic() {
        let habitat = island(64);
        let a = seed_agents(64, 200, &habitat, HAB_MIN).expect("habitable");
        let b = seed_agents(64, 200, &habitat, HAB_MIN).expect("habitable");
        assert_eq!(a, b, "same seed must produce an identical scatter");
    }

    #[test]
    fn a_dungeon_with_no_habitat_fails_loudly() {
        let err = seed_agents(4, 10, &[0u8; 16], HAB_MIN).expect_err("no habitat must be an error");
        assert!(err.contains("no habitable texel"), "unhelpful error: {err}");
    }

    #[test]
    fn a_mask_of_the_wrong_size_fails_loudly() {
        let err = seed_agents(4, 10, &[255u8; 15], HAB_MIN).expect_err("bad mask must be an error");
        assert!(err.contains("expected 4x4"), "unhelpful error: {err}");
    }

    /// Agents must spread over all of the habitat, not clump in one texel.
    #[test]
    fn agents_cover_the_whole_habitat() {
        let field = 64u32;
        let habitat = island(field);
        let data = seed_agents(field, 20_000, &habitat, HAB_MIN).expect("habitable");

        let mut hit = std::collections::HashSet::new();
        for (x, y) in positions(&data) {
            hit.insert((x.floor() as u32, y.floor() as u32));
        }
        assert_eq!(hit.len(), 16 * 16, "expected every habitable texel occupied, got {}", hit.len());
    }
}
