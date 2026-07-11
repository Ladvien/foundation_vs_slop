//! Crabs eat the mushrooms — but only where nobody is looking.
//!
//! The mold conceals its glow from a gaze. Its fruit body is the inverse: being *watched* is what protects
//! it. A player who stares at a flush keeps it; a player who turns away comes back to bare stipes and a fat
//! crab. Nothing is ever seen shrinking, so the exemption at [`FruitBody::consume`] — "being eaten is meant
//! to be seen" — never actually fires, and the perceptual speed limit is not violated by the back door.
//!
//! # This is the seam `fruit.rs` predicted, and it needed no AI changes
//!
//! Crabs already forage the [`FieldId::MEAT`] stigmergy field against an accumulating [`DriveId::HUNGER`],
//! scored by `Mode::SeekMeat` / `TargetKind::MeatHotspot`, and they already sate that hunger by biting what
//! they reach. All that was missing was for a mature cap to *smell* like food. So [`deposit_fruit_scent`]
//! splats it into the same field gore uses, and the existing utility brain does the rest.
//!
//! # Why this module exists at all, instead of living in `crab.rs`
//!
//! These two systems run on **`FixedUpdate`** — they steer crab `Transform`s, which `sim_harness::snapshot_hash`
//! reads, so by the repo's own rule (see `TESTING.md`) they are pinned state, not cosmetics. That would
//! normally be forbidden here: fruit-body *positions* come from a GPU readback and are not bit-reproducible
//! across hardware.
//!
//! It is safe because `MyceliaPlugin` is registered **only** in `lib::run`, never in
//! `sim_harness::build_headless_app_unfinished`. The headless replay harness spawns no fruit bodies, runs
//! neither of these systems, and cannot diverge. `CrabPlugin` *is* registered there — which is exactly why
//! this code must not live in `crab.rs`. The determinism firewall is a plugin boundary, not a property of
//! these systems, and moving them one file over would quietly breach it.
//!
//! Amatoxin is deliberately not modelled: [`FruitBody::amatoxin`] stays an unused hook. A death cap that
//! killed the swarm would be a defence, and this is meant to be an ecosystem, not a trap.

use bevy::prelude::*;

use crate::ai::drives::{DriveId, Drives};
use crate::ai::field::{Deposit, FieldId, StigDeposits};
use crate::ai::AiSet;
use crate::crab::Crab;
use crate::fog::FogGrid;

use super::fruit::FruitBody;
use super::perceptual::VEIL_RUPTURE_T;

/// Scent a mature, unwatched cap sheds into [`FieldId::MEAT`] per second.
///
/// A fifth of the rate a gib chunk gives off (`crab::MEAT_RATE` is `0.5`). Carrion outranks fungus: a crab
/// that can smell a corpse should go to the corpse. A mushroom is what it eats when there is nothing better,
/// which is also when the player is least likely to be watching the room.
const FRUIT_MEAT_RATE: f32 = 0.1;

/// Planar reach, in world units, at which a crab is close enough to bite a body. The adult cap spans
/// `perceptual::CAP_RADIUS_M * body_scale` — about 22 cm at the shipped scale — so a crab standing under the
/// rim is feeding.
const GRAZE_REACH: f32 = 0.35;

/// `growth` consumed per crab per second. A single crab strips a mature body in about seven seconds; a pile
/// does it in one. Deliberately fast — unlike growth, eating is not held under any perceptual threshold,
/// because eating never happens where it can be seen.
const GRAZE_BITE_RATE: f32 = 0.15;

pub(super) fn build(app: &mut App) {
    app.add_systems(
        FixedUpdate,
        (
            deposit_fruit_scent.before(AiSet::Deposits),
            crabs_graze_fruit_bodies.after(AiSet::Think),
        ),
    );
}

/// A mature cap standing in an unwatched room smells like food.
///
/// Only past [`VEIL_RUPTURE_T`]: below it the body is a sealed egg, mostly volva and water, and
/// [`FruitBody::energy`] agrees. Only where `!fog.visible_at`: the squad's gaze is the mushroom's protection.
///
/// **Emitted in sorted position order**, exactly as `crab::deposit_meat_scent` is and for the same reason:
/// deposits spread over a disc, so overlapping sources accumulate into shared cells, `f32 +=` is not
/// associative, and an unstable entity iteration order would make the summed gradient — and therefore the
/// swarm's steering — depend on ECS storage layout.
fn deposit_fruit_scent(
    time: Res<Time>,
    cfg: Res<super::MyceliaConfig>,
    fog: Res<FogGrid>,
    bodies: Query<(&Transform, &FruitBody)>,
    mut deposits: ResMut<StigDeposits>,
) {
    let dt = time.delta_secs();
    // A mushroom smells of food in proportion to how edible it is: a nutritious edible (oyster, bolete)
    // draws crabs, a poisonous one (death cap, destroying angel) barely registers — so the swarm learns
    // to avoid the amanitas without ever being poisoned. Toxicity is scaled by maturity, since the toxin
    // rides the expanded cap and gills (`FruitBody::amatoxin`).
    let mut out: Vec<(Vec3, f32)> = bodies
        .iter()
        .filter(|(_, b)| b.growth >= VEIL_RUPTURE_T && !fog.visible_at(b.cell))
        .map(|(tf, b)| {
            let sc = &cfg.species[b.species.0 as usize];
            let appetite = (sc.nutrition * (1.0 - sc.toxicity * b.amatoxin())).max(0.0);
            (tf.translation, FRUIT_MEAT_RATE * appetite * dt)
        })
        .filter(|(_, a)| *a > 0.0)
        .collect();
    out.sort_unstable_by_key(|(p, _)| (p.x.to_bits(), p.y.to_bits(), p.z.to_bits()));
    for (pos, amount) in out {
        deposits.0.push(Deposit { pos, field: FieldId::MEAT, amount });
    }
}

/// A crab within reach of an unwatched body takes a bite, and is fed by it.
///
/// [`FruitBody::consume`] runs the growth clock backwards along the same path primordium abortion uses.
/// There is no "eaten" state and no second despawn path — a body being eaten is a body ungrowing, which is
/// what a fungus being eaten actually is.
///
/// So a cropped body does **not** vanish. It is chewed back to a sealed egg, and if the mat beneath it is
/// still thick (`local_v >= maintain_v`) the knot simply pushes it up again over the following minutes. It
/// disappears only when the patch has collapsed too, and then by `fruit::grow_fruit_bodies`' one existing
/// reabsorption branch. That is the mycelium's whole strategy: the fruit body is expendable, the mat is not.
///
/// The gaze gate is re-checked here, not just at the scent: a squad that walks in on a feeding crab
/// interrupts it.
///
/// # Order independence
///
/// Both loops are written so the result cannot depend on ECS iteration order, which is not stable across
/// runs. A crab feeding at two bodies in one tick would otherwise drain `HUNGER` as `(h - a) - b`, and `f32`
/// subtraction is not associative — the two bodies' order would perturb its next utility decision, and from
/// there its `Transform`. So each crab's bites are **counted** (an integer, exactly order-independent) and
/// its hunger drained once. Each body's `growth` is likewise decremented once, by a count.
fn crabs_graze_fruit_bodies(
    time: Res<Time>,
    cfg: Res<super::MyceliaConfig>,
    fog: Res<FogGrid>,
    mut bodies: Query<(&Transform, &mut FruitBody)>,
    mut crabs: Query<(Entity, &Transform, &mut Drives), With<Crab>>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    // `crab::HUNGER_SATE_RATE`, mirrored: a grazing crab is a feeding crab.
    const HUNGER_SATE_RATE: f32 = 0.3;
    let reach_sq = GRAZE_REACH * GRAZE_REACH;

    let crab_positions: Vec<(Entity, Vec3)> =
        crabs.iter().map(|(entity, tf, _)| (entity, tf.translation)).collect();
    // Accumulate *food value* per crab, not bite count: a mouthful of oyster sates; a mouthful of a
    // mature death cap is toxin, not food, and sates almost nothing (`nutrition · (1 − toxicity·maturity)`).
    let mut sated: Vec<(Entity, f32)> = Vec::new();

    for (body_tf, mut body) in &mut bodies {
        if body.growth <= 0.0 || fog.visible_at(body.cell) {
            continue;
        }
        let sc = &cfg.species[body.species.0 as usize];
        let food = (sc.nutrition * (1.0 - sc.toxicity * body.amatoxin())).max(0.0);
        let mut biters = 0u32;
        for (entity, crab_pos) in &crab_positions {
            let d = *crab_pos - body_tf.translation;
            if Vec2::new(d.x, d.z).length_squared() > reach_sq {
                continue;
            }
            biters += 1;
            match sated.iter_mut().find(|(e, _)| e == entity) {
                Some((_, f)) => *f += food,
                None => sated.push((*entity, food)),
            }
        }
        if biters > 0 {
            // Linear in the pile, unlike `crab_contact_damage`'s super-linear frenzy: a mushroom is a fixed
            // quantity of food, not a body whose defences collapse under a swarm. The body is still consumed
            // whatever its toxicity — the crab takes the bite; it just gains nothing from a poisonous one.
            body.consume(GRAZE_BITE_RATE * biters as f32 * dt);
        }
    }

    for (entity, food) in sated {
        if let Ok((_, _, mut drives)) = crabs.get_mut(entity) {
            let h = drives.get(DriveId::HUNGER);
            drives.set(DriveId::HUNGER, h - HUNGER_SATE_RATE * food * dt);
        }
    }
}
