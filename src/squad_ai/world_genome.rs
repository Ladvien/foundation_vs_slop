//! The **world genome**: the game's evolvable *world-dynamics* config viewed as a flat parameter vector.
//!
//! Sibling of [`super::genome`] (which searches the squad/swarm *brains*). Where that one searches an
//! authored dual-utility repertoire, this one searches the numeric config the world runs on — the
//! field-propagation tuning ([`AiTuning`]) plus the simulation-dynamics tuning ([`SimTuning`]) — so the
//! offline search can evolve *worlds*, not just how agents weight authored actions (Wang et al., POET,
//! arXiv:1901.01753: co-evolving environments and their solutions).
//!
//! Two properties are kept deliberately, mirroring `genome`:
//!
//! 1. **Readable elites.** A [`WorldGenome`] decodes to a [`WorldConfig`] whose two slices serialise to the
//!    same RON a designer edits — so an elite is a *diff of world dials*, not opaque weights. That
//!    readability is the practical answer to reward hacking (Skalse et al., arXiv:2209.13085): you can read
//!    what the optimiser found and reject it.
//! 2. **Feasibility by construction.** Unlike the brain genome's authored-band + sign-lock, config knobs
//!    have *physical* bounds (an evaporation rate near 0 saturates the field; a diffusion weight ≥ 1 blows
//!    up the unclamped blur; a huge deposit radius floods the map). So mutation clamps every knob to a hard
//!    per-parameter [`BOUNDS`] table — the primary feasibility layer (Skalse's "restrict the admissible
//!    set"). Children are feasible without rejection sampling.
//!
//! **Mutation is scale-relative Gaussian**, the same kernel as the brain genome: each knob is kicked by
//! `N(0, sigma·(|authored| + SCALE_FLOOR))` and clamped to its `BOUNDS` — so the search explores in units
//! proportional to the shipped value, but can never leave the playable range.

use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};

use crate::ai::tuning::{AiTuning, ChannelTuning, FieldsTuning, RallyTuning};
use crate::config::WorldConfig;
use crate::sim::{BossTuning, BreedingTuning, CombatTuning, DepositTuning, FearTuning, SimTuning};

/// Number of knobs: 27 field-propagation (`AiTuning`: 8 channels × {evaporate, diffuse, deposit_radius}
/// + rally × 3) + 37 simulation-dynamics (`SimTuning`: fear 3, deposit 9, combat 9, breeding 9, boss 7).
pub const N: usize = 64;

/// Minimum mutation scale, so a knob authored at (or near) `0.0` — e.g. `diffuse` — can still move.
/// Smaller than the brain genome's `0.25`: world knobs span a wider range of magnitudes and a large floor
/// would swamp the small ones (fear gains ~0.08, exponents ~1.5).
const SCALE_FLOOR: f32 = 0.05;

/// Hard `(min, max)` per knob, in the **same order** as [`encode`] walks the config. Each shipped value
/// sits comfortably inside its range; the extremes are playable-but-different, never degenerate. This
/// table IS the primary feasibility gate — `evaporate` floored at `0.05` (never saturating), `diffuse`
/// capped at `0.6` (the blur lerp weight must stay `< 1`), `deposit_radius` capped so a deposit can't
/// flood the whole map. Integer knobs (population cap, cull counts) carry float bounds; [`decode`] rounds.
static BOUNDS: [(f32, f32); N] = [
    // ── AiTuning: 8 stigmergy channels × (evaporate, diffuse, deposit_radius) ──
    (0.05, 3.0), (0.0, 0.6), (0.5, 8.0), // scent
    (0.05, 3.0), (0.0, 0.6), (0.5, 8.0), // threat_gun
    (0.05, 3.0), (0.0, 0.6), (0.5, 8.0), // crab_density
    (0.05, 3.0), (0.0, 0.6), (0.5, 8.0), // meat
    (0.05, 3.0), (0.0, 0.6), (0.5, 8.0), // alarm
    (0.05, 3.0), (0.0, 0.6), (0.5, 8.0), // threat_crab
    (0.05, 3.0), (0.0, 0.6), (0.5, 8.0), // threat_anomaly
    (0.05, 3.0), (0.0, 0.6), (0.5, 8.0), // attention
    // rally (decay, accumulate, deposit_radius)
    (0.05, 3.0), (0.05, 3.0), (0.5, 8.0),
    // ── SimTuning::fear (per_crab, of_anomaly, crab_of_gunfire) ──
    (0.01, 0.5), (0.1, 1.0), (0.01, 1.0),
    // ── SimTuning::deposit ──
    (0.05, 3.0),  // threat_per_shot
    (0.5, 12.0),  // blood_scent
    (0.05, 3.0),  // crab_density_rate
    (0.05, 3.0),  // crab_menace_rate
    (0.05, 3.0),  // meat_rate
    (0.05, 3.0),  // anomaly_aura_rate
    (0.2, 8.0),   // alarm_crab
    (0.2, 12.0),  // alarm_nest
    (0.2, 12.0),  // rally_mark
    // ── SimTuning::combat ──
    (1.0, 50.0),  // laser_damage
    (0.0, 1.0),   // friendly_fire_chance
    (0.0, 25.0),  // friendly_fire_damage
    (0.5, 15.0),  // crab_contact_dps
    (1.0, 2.5),   // crab_damage_exponent
    (1.0, 30.0),  // crab_jump_damage
    (5.0, 100.0), // crab_hp
    (25.0, 300.0),// unit_hp
    (0.0, 1.0),   // crab_drag
    // ── SimTuning::breeding ──
    (40.0, 400.0),// crab_count_max (usize)
    (1.0, 20.0),  // respawn_interval
    (0.25, 5.0),  // meat_per_crab
    (1.0, 20.0),  // feed_gain
    (1.0, 30.0),  // spawn_boost_max
    (0.1, 5.0),   // spawn_boost_decay
    (1.0, 20.0),  // crowd_cap
    (0.005, 0.3), // hunger_rate
    (0.05, 2.0),  // hunger_sate_rate
    // ── SimTuning::boss ──
    (400.0, 6000.0), // start_hp
    (0.2, 6.0),      // scared_time
    (0.1, 3.0),      // zap_cadence
    (1.0, 20.0),     // cull_threshold (usize)
    (0.5, 5.0),      // cull_radius
    (1.0, 30.0),     // cull_max (usize)
    (0.5, 10.0),     // cull_cooldown
];

/// A world's evolvable config, flattened. Meaningless without [`BOUNDS`]/[`decode`], which pin the layout.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorldGenome(pub Vec<f32>);

fn push_channel(v: &mut Vec<f32>, c: &ChannelTuning) {
    v.push(c.evaporate);
    v.push(c.diffuse);
    v.push(c.deposit_radius);
}

/// Flatten a `(AiTuning, SimTuning)` into the fixed-order knob vector `BOUNDS` and `decode` agree on.
pub fn encode(ai: &AiTuning, sim: &SimTuning) -> WorldGenome {
    let mut v = Vec::with_capacity(N);
    // AiTuning: channels in FieldId slot order, then rally.
    push_channel(&mut v, &ai.fields.scent);
    push_channel(&mut v, &ai.fields.threat_gun);
    push_channel(&mut v, &ai.fields.crab_density);
    push_channel(&mut v, &ai.fields.meat);
    push_channel(&mut v, &ai.fields.alarm);
    push_channel(&mut v, &ai.fields.threat_crab);
    push_channel(&mut v, &ai.fields.threat_anomaly);
    push_channel(&mut v, &ai.fields.attention);
    v.push(ai.rally.decay);
    v.push(ai.rally.accumulate);
    v.push(ai.rally.deposit_radius);
    // SimTuning.
    v.push(sim.fear.per_crab);
    v.push(sim.fear.of_anomaly);
    v.push(sim.fear.crab_of_gunfire);
    v.push(sim.deposit.threat_per_shot);
    v.push(sim.deposit.blood_scent);
    v.push(sim.deposit.crab_density_rate);
    v.push(sim.deposit.crab_menace_rate);
    v.push(sim.deposit.meat_rate);
    v.push(sim.deposit.anomaly_aura_rate);
    v.push(sim.deposit.alarm_crab);
    v.push(sim.deposit.alarm_nest);
    v.push(sim.deposit.rally_mark);
    v.push(sim.combat.laser_damage);
    v.push(sim.combat.friendly_fire_chance);
    v.push(sim.combat.friendly_fire_damage);
    v.push(sim.combat.crab_contact_dps);
    v.push(sim.combat.crab_damage_exponent);
    v.push(sim.combat.crab_jump_damage);
    v.push(sim.combat.crab_hp);
    v.push(sim.combat.unit_hp);
    v.push(sim.combat.crab_drag);
    v.push(sim.breeding.crab_count_max as f32);
    v.push(sim.breeding.respawn_interval);
    v.push(sim.breeding.meat_per_crab);
    v.push(sim.breeding.feed_gain);
    v.push(sim.breeding.spawn_boost_max);
    v.push(sim.breeding.spawn_boost_decay);
    v.push(sim.breeding.crowd_cap);
    v.push(sim.breeding.hunger_rate);
    v.push(sim.breeding.hunger_sate_rate);
    v.push(sim.boss.start_hp);
    v.push(sim.boss.scared_time);
    v.push(sim.boss.zap_cadence);
    v.push(sim.boss.cull_threshold as f32);
    v.push(sim.boss.cull_radius);
    v.push(sim.boss.cull_max as f32);
    v.push(sim.boss.cull_cooldown);
    debug_assert_eq!(v.len(), N, "encode walked the wrong number of knobs");
    WorldGenome(v)
}

/// Round a genome float back to a positive integer knob (population cap / cull count). `>= 1`, saturating.
fn to_usize(x: f32) -> usize {
    x.round().max(1.0) as usize
}

/// Rebuild a `WorldConfig` from the flat vector. `Err` on wrong length — one path, no padding/truncation.
/// The struct-literal fields are written in the same order [`encode`] pushes; the round-trip test pins it.
pub fn decode(g: &WorldGenome) -> Result<WorldConfig, String> {
    if g.0.len() != N {
        return Err(format!("world genome has {} knobs, expected {N}", g.0.len()));
    }
    let v = &g.0;
    let mut i = 0usize;
    // Reads `v[i]` and advances. `i` never reaches the len (exactly `N` reads below, guarded above), so no
    // out-of-range access. Rust evaluates struct-literal fields left-to-right as written, so the reads bind
    // to the fields in `encode` order.
    macro_rules! f {
        () => {{
            let x = v[i];
            i += 1;
            x
        }};
    }
    let ai = AiTuning {
        fields: FieldsTuning {
            scent: ChannelTuning { evaporate: f!(), diffuse: f!(), deposit_radius: f!() },
            threat_gun: ChannelTuning { evaporate: f!(), diffuse: f!(), deposit_radius: f!() },
            crab_density: ChannelTuning { evaporate: f!(), diffuse: f!(), deposit_radius: f!() },
            meat: ChannelTuning { evaporate: f!(), diffuse: f!(), deposit_radius: f!() },
            alarm: ChannelTuning { evaporate: f!(), diffuse: f!(), deposit_radius: f!() },
            threat_crab: ChannelTuning { evaporate: f!(), diffuse: f!(), deposit_radius: f!() },
            threat_anomaly: ChannelTuning { evaporate: f!(), diffuse: f!(), deposit_radius: f!() },
            attention: ChannelTuning { evaporate: f!(), diffuse: f!(), deposit_radius: f!() },
        },
        rally: RallyTuning { decay: f!(), accumulate: f!(), deposit_radius: f!() },
    };
    let sim = SimTuning {
        fear: FearTuning { per_crab: f!(), of_anomaly: f!(), crab_of_gunfire: f!() },
        deposit: DepositTuning {
            threat_per_shot: f!(),
            blood_scent: f!(),
            crab_density_rate: f!(),
            crab_menace_rate: f!(),
            meat_rate: f!(),
            anomaly_aura_rate: f!(),
            alarm_crab: f!(),
            alarm_nest: f!(),
            rally_mark: f!(),
        },
        combat: CombatTuning {
            laser_damage: f!(),
            friendly_fire_chance: f!(),
            friendly_fire_damage: f!(),
            crab_contact_dps: f!(),
            crab_damage_exponent: f!(),
            crab_jump_damage: f!(),
            crab_hp: f!(),
            unit_hp: f!(),
            crab_drag: f!(),
        },
        breeding: BreedingTuning {
            crab_count_max: to_usize(f!()),
            respawn_interval: f!(),
            meat_per_crab: f!(),
            feed_gain: f!(),
            spawn_boost_max: f!(),
            spawn_boost_decay: f!(),
            crowd_cap: f!(),
            hunger_rate: f!(),
            hunger_sate_rate: f!(),
        },
        boss: BossTuning {
            start_hp: f!(),
            scared_time: f!(),
            zap_cadence: f!(),
            cull_threshold: to_usize(f!()),
            cull_radius: f!(),
            cull_max: to_usize(f!()),
            cull_cooldown: f!(),
        },
        // The SCP-150 parasite knobs are not part of the evolved genome yet — the world search leaves them
        // at their shipped defaults (so encode/decode round-trips at the same genome length `N`).
        parasite: SimTuning::default().parasite,
    };
    debug_assert_eq!(i, N, "decode read the wrong number of knobs");
    Ok(WorldConfig { ai, sim })
}

/// The shipped world as a genome — the band origin for [`mutate`] and the co-evolution's baseline.
pub fn authored() -> WorldGenome {
    encode(&AiTuning::default(), &SimTuning::default())
}

/// Perturb a world genome: every knob gets a scale-relative Gaussian kick (scale from the **authored**
/// value, so bands can't ratchet across generations — the anti-drift rule of `genome::mutate`), clamped to
/// its hard [`BOUNDS`]. Because the clamp is the physical range, children are feasible by construction —
/// `propose_world` is one `mutate`, no rejection loop. Reuses `genome::gaussian` (one Gaussian kernel).
pub fn mutate(parent: &WorldGenome, sigma: f32, rng: &mut ChaCha8Rng) -> Result<WorldGenome, String> {
    if parent.0.len() != N {
        return Err(format!("world genome has {} knobs, expected {N}", parent.0.len()));
    }
    let authored = authored();
    let mut out = Vec::with_capacity(N);
    for i in 0..N {
        let origin = authored.0[i];
        let scale = origin.abs() + SCALE_FLOOR;
        let moved = parent.0[i] + super::genome::gaussian(rng) * sigma * scale;
        let (lo, hi) = BOUNDS[i];
        out.push(moved.clamp(lo, hi));
    }
    Ok(WorldGenome(out))
}

/// The genome-level feasibility gate: right length, every knob finite and within [`BOUNDS`], and the
/// decoded `SimTuning` passes `sim::validate_tuning`. `mutate` guarantees this by construction; the check
/// exists for genomes built any other way (e.g. loaded from a committed archive). One `Err`, no fallback.
pub fn is_feasible(g: &WorldGenome) -> Result<(), String> {
    if g.0.len() != N {
        return Err(format!("world genome has {} knobs, expected {N}", g.0.len()));
    }
    for (i, &x) in g.0.iter().enumerate() {
        let (lo, hi) = BOUNDS[i];
        if !(x.is_finite() && (lo..=hi).contains(&x)) {
            return Err(format!("world genome knob {i} = {x} is out of bounds [{lo}, {hi}]"));
        }
    }
    let wc = decode(g)?;
    crate::sim::validate_tuning(&wc.sim)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::seeded;

    #[test]
    fn authored_round_trips_exactly() {
        // encode ∘ decode is the identity on the shipped config — pins that the encode/decode field walks
        // agree, and that the usize knobs survive the f32 round trip.
        let wc = decode(&authored()).expect("authored decodes");
        assert_eq!(wc.ai, AiTuning::default());
        assert_eq!(wc.sim, SimTuning::default());
    }

    #[test]
    fn authored_has_n_knobs_and_sits_inside_bounds() {
        let g = authored();
        assert_eq!(g.0.len(), N, "authored genome length");
        for (i, &x) in g.0.iter().enumerate() {
            let (lo, hi) = BOUNDS[i];
            assert!(
                (lo..=hi).contains(&x),
                "shipped knob {i} = {x} is not within its BOUNDS [{lo}, {hi}] — the search could never \
                 reach the shipped value, or the bounds are miscalibrated"
            );
        }
        is_feasible(&g).expect("the shipped world must be feasible");
    }

    #[test]
    fn mutation_stays_within_bounds_and_finite() {
        let authored = authored();
        let mut rng = seeded(0x5EED);
        for _ in 0..500 {
            let child = mutate(&authored, 0.3, &mut rng).expect("mutate");
            assert_eq!(child.0.len(), N);
            for (i, &x) in child.0.iter().enumerate() {
                let (lo, hi) = BOUNDS[i];
                assert!(x.is_finite(), "knob {i} became non-finite");
                assert!((lo..=hi).contains(&x), "knob {i} = {x} escaped BOUNDS [{lo}, {hi}]");
            }
            is_feasible(&child).expect("a clamped child must be feasible by construction");
        }
    }

    #[test]
    fn a_mutation_actually_moves_something() {
        // Guard against a σ so small (or a frozen scale) that mutation is a no-op — which would make the
        // world search stand still.
        let authored = authored();
        let mut rng = seeded(0xC0DE);
        let child = mutate(&authored, 0.3, &mut rng).expect("mutate");
        assert_ne!(child, authored, "mutation changed nothing");
    }

    #[test]
    fn decode_rejects_a_wrong_length_genome() {
        assert!(decode(&WorldGenome(vec![0.0; N - 1])).is_err());
        assert!(is_feasible(&WorldGenome(vec![0.0; N + 1])).is_err());
    }
}
