//! The Physarum agent population — the "mind" of the mold. Each agent is a point walker on the trail
//! field that senses trail density ahead-left/ahead-right, steers toward the stronger scent, steps, and
//! deposits a fresh scent mark. Thousands of these following Jones' three-sensor rule self-organize into
//! the branching, foraging transport network that reads as *intelligence*.
//!
//! Ref: Jones (2010), "Characteristics of pattern formation and evolution in approximations of Physarum
//! transport networks," *Artificial Life* (arXiv 1503.06579) — the sense→rotate→move→deposit→diffuse→decay
//! loop every real-time GPU Physarum uses (Jenson, Lague).
//!
//! # GPU layout
//! Seeded on the CPU (deterministically, via [`crate::rng::seeded`]) then handed to the GPU as a
//! `ShaderBuffer`. We encode each agent as four `u32`s — the raw bits of `pos.x`, `pos.y`, `heading`, and a
//! pad word — so `ShaderBuffer::from(Vec<u32>)` lays them out as a tightly-packed `array<u32>` whose bytes
//! are exactly a std430 `array<Agent>` (`Agent { pos: vec2<f32>, heading: f32, _pad: f32 }`, stride 16).
//! GPU float drift *after* seeding is accepted (the whole layer is cosmetic; see `mod.rs` firewall notes).

use rand::RngExt;

use crate::rng;

/// Number of walking agents. Kept deliberately sparse (≈0.05 agents/texel over the 1024² field) so the
/// trail forms legible foraging *channels* rather than flooding to uniform saturation — the network reads
/// as veins, not a solid film. The GPU can handle far more (2M+ at 120 FPS on a mid-range card,
/// rechenwerke.com), so this is an aesthetic ceiling, not a performance one; it graduates to config later.
pub const AGENT_COUNT: u32 = 55_000;

/// Fixed seed for the initial scatter. The mold's opening arrangement is identical every run; divergence
/// afterward is GPU-side and cosmetic.
const AGENT_SEED: u64 = 0x_C0FFEE_5EED_0FED;

/// Seed `AGENT_COUNT` agents scattered uniformly across the `field_size`×`field_size` field with random
/// headings, encoded as the flat `u32` bit-buffer described in the module docs (4 words per agent).
pub fn seed_agents(field_size: f32) -> Vec<u32> {
    let mut rng = rng::seeded(AGENT_SEED);
    let mut data = Vec::with_capacity(AGENT_COUNT as usize * 4);
    for _ in 0..AGENT_COUNT {
        let x = rng.random::<f32>() * field_size;
        let y = rng.random::<f32>() * field_size;
        let heading = rng.random::<f32>() * std::f32::consts::TAU;
        data.push(x.to_bits());
        data.push(y.to_bits());
        data.push(heading.to_bits());
        data.push(0u32); // _pad — keeps the per-agent stride at 16 bytes (std430 `Agent`).
    }
    data
}
