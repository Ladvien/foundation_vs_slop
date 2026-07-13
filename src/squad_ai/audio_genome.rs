//! The **audio genome**: the game's evolvable *acoustic-stimulus* config viewed as a flat parameter
//! vector — the audio-population sibling of [`super::world_genome`].
//!
//! Where the world genome searches the field-propagation + simulation-dynamics numbers, this one searches
//! the [`AudioTuning`] slice: how far each acoustic channel (`NOISE_SQUAD`/`NOISE_SWARM`) carries, how
//! loud each event is *as a stimulus*, and how strongly each faction reacts to the other's din (fear
//! gains, and the investigate draw + threshold that decide whether the swarm runs from the guns or toward
//! them). Because sound feeds back into agent perception (`ai::field` acoustic channels →
//! `ai::drives`/`ai::brain`), evolving these knobs searches for **novel emergent agent behaviour** the
//! same full-simulation surprise fitness (`super::surprise`) measures for the world/brain populations.
//!
//! Same two properties as `world_genome`:
//! 1. **Readable elites** — a [`AudioGenome`] decodes to an [`AudioTuning`] that serialises to the RON a
//!    designer edits, so an elite is a diff of audio dials (the reward-hacking guard, Skalse et al.
//!    arXiv:2209.13085).
//! 2. **Feasibility by construction** — every knob is clamped to a hard per-parameter [`BOUNDS`] table
//!    (channel evaporate never saturating, diffuse `< 1`, radii bounded so a deposit can't flood the map),
//!    so mutation needs no rejection loop.
//!
//! Mutation is the same scale-relative Gaussian kernel as `world_genome`/`genome`: `N(0, sigma·(|authored|
//! + SCALE_FLOOR))` clamped to `BOUNDS`. Note `crab_draw_to_din` ships at `0.0` (the investigate behaviour
//! is dormant), and the `SCALE_FLOOR` is exactly what lets the search lift it off zero and turn the
//! swarm-toward-the-guns behaviour on.

use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};

use super::genome::{flat_mutate, flat_range_check, push_channel};
use crate::ai::tuning::ChannelTuning;
use crate::audio_tuning::{AcousticPerceptionTuning, AcousticStimulusTuning, AudioTuning};

/// Number of knobs: 6 propagation (2 acoustic channels × {evaporate, diffuse, deposit_radius}) + 5
/// per-event loudness (deposit amounts) + 4 perception (crab/unit din-fear, draw, investigate threshold).
pub const N: usize = 15;

/// Hard `(min, max)` per knob, in the **same order** [`encode`] walks [`AudioTuning`]. Channel bounds match
/// `world_genome` (evaporate never saturating, diffuse `< 1`, radius bounded); loudness is a deposit
/// amount (`0` = silent stimulus is a legitimate point); the din-fear gains stay small like `SimTuning`'s
/// fear gains; `crab_draw_to_din` spans `[0, 1]` (0 = no convergence, 1 = strong pull); the investigate
/// threshold is a field-value gate. This table IS the primary feasibility gate.
static BOUNDS: [(f32, f32); N] = [
    // ── AcousticStimulusTuning: NOISE_SQUAD (evaporate, diffuse, deposit_radius) ──
    (0.05, 3.0), (0.0, 0.6), (0.5, 8.0),
    // ── NOISE_SWARM (evaporate, diffuse, deposit_radius) ──
    (0.05, 3.0), (0.0, 0.6), (0.5, 8.0),
    // ── per-event loudness (deposit amounts) ──
    (0.0, 3.0), // fire_loudness
    (0.0, 3.0), // impact_wall_loudness
    (0.0, 3.0), // impact_flesh_loudness
    (0.0, 3.0), // enemy_death_loudness
    (0.0, 3.0), // unit_death_loudness
    // ── AcousticPerceptionTuning ──
    (0.0, 0.5), // crab_fear_of_din
    (0.0, 0.5), // unit_fear_of_din
    (0.0, 1.0), // crab_draw_to_din
    (0.05, 3.0), // investigate_threshold
];

/// An audio config, flattened. Meaningless without [`BOUNDS`]/[`decode`], which pin the layout.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AudioGenome(pub Vec<f32>);

/// Flatten an [`AudioTuning`] into the fixed-order knob vector `BOUNDS` and `decode` agree on.
pub fn encode(a: &AudioTuning) -> AudioGenome {
    let mut v = Vec::with_capacity(N);
    push_channel(&mut v, &a.stimulus.noise_squad);
    push_channel(&mut v, &a.stimulus.noise_swarm);
    v.push(a.stimulus.fire_loudness);
    v.push(a.stimulus.impact_wall_loudness);
    v.push(a.stimulus.impact_flesh_loudness);
    v.push(a.stimulus.enemy_death_loudness);
    v.push(a.stimulus.unit_death_loudness);
    v.push(a.perception.crab_fear_of_din);
    v.push(a.perception.unit_fear_of_din);
    v.push(a.perception.crab_draw_to_din);
    v.push(a.perception.investigate_threshold);
    debug_assert_eq!(v.len(), N, "encode walked the wrong number of knobs");
    AudioGenome(v)
}

/// Rebuild an [`AudioTuning`] from the flat vector. `Err` on wrong length — one path, no padding. The
/// struct-literal fields are written in the same order [`encode`] pushes; the round-trip test pins it.
pub fn decode(g: &AudioGenome) -> Result<AudioTuning, String> {
    if g.0.len() != N {
        return Err(format!("audio genome has {} knobs, expected {N}", g.0.len()));
    }
    let v = &g.0;
    let mut i = 0usize;
    // Reads `v[i]` and advances; exactly `N` reads below, guarded above. Rust evaluates struct-literal
    // fields left-to-right as written, so the reads bind to the fields in `encode` order.
    macro_rules! f {
        () => {{
            let x = v[i];
            i += 1;
            x
        }};
    }
    let audio = AudioTuning {
        stimulus: AcousticStimulusTuning {
            noise_squad: ChannelTuning { evaporate: f!(), diffuse: f!(), deposit_radius: f!() },
            noise_swarm: ChannelTuning { evaporate: f!(), diffuse: f!(), deposit_radius: f!() },
            fire_loudness: f!(),
            impact_wall_loudness: f!(),
            impact_flesh_loudness: f!(),
            enemy_death_loudness: f!(),
            unit_death_loudness: f!(),
        },
        perception: AcousticPerceptionTuning {
            crab_fear_of_din: f!(),
            unit_fear_of_din: f!(),
            crab_draw_to_din: f!(),
            investigate_threshold: f!(),
        },
    };
    debug_assert_eq!(i, N, "decode read the wrong number of knobs");
    Ok(audio)
}

/// The shipped audio as a genome — the band origin for [`mutate`] and the search's baseline.
pub fn authored() -> AudioGenome {
    encode(&AudioTuning::default())
}

/// Perturb an audio genome: every knob gets a scale-relative Gaussian kick (scale from the **authored**
/// value, so bands can't ratchet across generations), clamped to its hard [`BOUNDS`]. Because the clamp is
/// the physical range, children are feasible by construction — no rejection loop. Reuses `genome::gaussian`.
pub fn mutate(parent: &AudioGenome, sigma: f32, rng: &mut ChaCha8Rng) -> Result<AudioGenome, String> {
    if parent.0.len() != N {
        return Err(format!("audio genome has {} knobs, expected {N}", parent.0.len()));
    }
    let authored = authored();
    Ok(AudioGenome(flat_mutate(&parent.0, &authored.0, &BOUNDS, sigma, rng)))
}

/// The genome-level feasibility gate: right length, every knob finite and within [`BOUNDS`], and the
/// decoded [`AudioTuning`] passes `audio_tuning::validate_tuning`. `mutate` guarantees this by
/// construction; the check exists for genomes built any other way (e.g. loaded from a committed archive).
pub fn is_feasible(g: &AudioGenome) -> Result<(), String> {
    if g.0.len() != N {
        return Err(format!("audio genome has {} knobs, expected {N}", g.0.len()));
    }
    flat_range_check(&g.0, &BOUNDS, "audio")?;
    let audio = decode(g)?;
    crate::audio_tuning::validate_tuning(&audio)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::seeded;

    #[test]
    fn authored_round_trips_exactly() {
        let a = decode(&authored()).expect("authored decodes");
        assert_eq!(a, AudioTuning::default());
    }

    #[test]
    fn authored_has_n_knobs_and_sits_inside_bounds() {
        let g = authored();
        assert_eq!(g.0.len(), N, "authored genome length");
        for (i, &x) in g.0.iter().enumerate() {
            let (lo, hi) = BOUNDS[i];
            assert!(
                (lo..=hi).contains(&x),
                "shipped audio knob {i} = {x} is not within its BOUNDS [{lo}, {hi}]"
            );
        }
        is_feasible(&g).expect("the shipped audio config must be feasible");
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
    fn mutation_can_lift_the_dormant_investigate_draw_off_zero() {
        // `crab_draw_to_din` ships at 0.0 (investigate dormant). The SCALE_FLOOR must let the search move
        // it up — otherwise the whole converge-on-the-guns behaviour would be unreachable.
        let authored = authored();
        let mut rng = seeded(0xD00D);
        let draw_idx = N - 2; // crab_draw_to_din, per the BOUNDS/encode order
        let mut moved_up = false;
        for _ in 0..200 {
            let child = mutate(&authored, 0.5, &mut rng).expect("mutate");
            if child.0[draw_idx] > 0.05 {
                moved_up = true;
                break;
            }
        }
        assert!(moved_up, "mutation never lifted crab_draw_to_din off zero — investigate is unreachable");
    }

    #[test]
    fn decode_rejects_a_wrong_length_genome() {
        assert!(decode(&AudioGenome(vec![0.0; N - 1])).is_err());
        assert!(is_feasible(&AudioGenome(vec![0.0; N + 1])).is_err());
    }
}
