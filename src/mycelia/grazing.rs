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

// The grazing/deposit rates now live in the `behavior:` config slice
// (`BehaviorTuning::mycelia_coupling`, read via `Res<BehaviorTuning>` in each system below):
//   fruit_meat_rate — scent a mature cap sheds into MEAT (a fifth of a gib chunk's rate; carrion outranks
//     fungus, and the cap only smells when unwatched).
//   graze_reach     — planar world-unit reach at which a crab is close enough to bite a body.
//   graze_bite_rate — `growth` consumed per crab per second (deliberately fast; eating is unheld).
//   mat_dense_v     — biomass `V` above which a mold mat is thick enough to smell of food.
//   mat_meat_rate   — scent a thick MAT sheds into MEAT per second per unit biomass (below fruit_meat_rate,
//     so a fruit body always out-draws bare mat, yet a dense corridor still dens the swarm — coupled terrain).

pub(super) fn build(app: &mut App) {
    app.add_systems(
        FixedUpdate,
        (
            deposit_fruit_scent.before(AiSet::Deposits),
            // The mold read-edge: thick unwatched mats den the swarm (biomass → MEAT). Same plugin-boundary
            // determinism firewall as `deposit_fruit_scent` — `MyceliaPlugin` is never in the harness.
            mold_mat_scent.before(AiSet::Deposits),
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
    beh: Res<crate::behavior_tuning::BehaviorTuning>,
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
            (tf.translation, beh.mycelia_coupling.fruit_meat_rate * appetite * dt)
        })
        .filter(|(_, a)| *a > 0.0)
        .collect();
    // Whole value, not just the position: `(pos, amount)` keyed on pos alone was a PREFIX, so two
    // grazers on one spot with different amounts tied and fed `drain_deposits`' non-associative `+=` in ECS
    // query order. With `amount` in the key a tie means the deposits are identical ⇒ interchangeable.
    crate::util::sort_value_canonical(&mut out, |(p, a)| {
        (p.x.to_bits(), p.y.to_bits(), p.z.to_bits(), a.to_bits())
    });
    for (pos, amount) in out {
        deposits.0.push(Deposit { pos, field: FieldId::MEAT, amount });
    }
}

/// A thick, unwatched mold MAT smells faintly of food — the mold read-edge that turns the biomass field
/// from a decorative one-way island into coupled forage terrain, so the swarm dens onto dense corridors.
///
/// Same grammar as [`deposit_fruit_scent`]: only DENSE mat (biomass ≥ `mycelia_coupling.mat_dense_v`), only on the floor,
/// only where `!fog.visible_at` (the mold's concealment is its protection), and emitted in SORTED position
/// order (overlapping MEAT discs accumulate with non-associative `f32 +=`, so an unstable order would make
/// the summed gradient — and the swarm's steering — depend on storage layout). Bounded to the coarse
/// readback grid, and inert before the first GPU readback (`dense_cells` yields nothing while empty).
fn mold_mat_scent(
    time: Res<Time>,
    coarse: Res<super::fruit::MoldCoarse>,
    beh: Res<crate::behavior_tuning::BehaviorTuning>,
    fog: Res<FogGrid>,
    dungeon: Res<crate::dungeon::Dungeon>,
    mut deposits: ResMut<StigDeposits>,
) {
    let dt = time.delta_secs();
    let mut out: Vec<(Vec3, f32)> = coarse
        .dense_cells(beh.mycelia_coupling.mat_dense_v)
        .filter_map(|(xz, v)| {
            let pos = Vec3::new(xz.x, 0.0, xz.y);
            let cell = dungeon.world_to_cell(pos);
            (dungeon.is_floor(cell) && !fog.visible_at(cell)).then_some((pos, beh.mycelia_coupling.mat_meat_rate * v * dt))
        })
        .filter(|(_, a)| *a > 0.0)
        .collect();
    // Whole value, not just the position: `(pos, amount)` keyed on pos alone was a PREFIX, so two
    // grazers on one spot with different amounts tied and fed `drain_deposits`' non-associative `+=` in ECS
    // query order. With `amount` in the key a tie means the deposits are identical ⇒ interchangeable.
    crate::util::sort_value_canonical(&mut out, |(p, a)| {
        (p.x.to_bits(), p.y.to_bits(), p.z.to_bits(), a.to_bits())
    });
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
/// The result cannot depend on ECS iteration order, which is not stable across runs. Each body's `growth`
/// is decremented once by an integer bite **count** (exactly order-independent). Each crab's hunger is
/// drained once by the summed *food value* of its bites (`nutrition · (1 − toxicity·maturity)`); because
/// `f32` addition is not associative, that per-crab sum is made reproducible by collecting the bites and
/// summing them in a fixed (sorted) order — never in raw body-visit order — before the single `HUNGER`
/// drain. A crab feeding at two bodies in one tick therefore drains the same amount whatever order the
/// query walked the bodies, so its next utility decision (and from there its `Transform`) is stable.
fn crabs_graze_fruit_bodies(
    time: Res<Time>,
    cfg: Res<super::MyceliaConfig>,
    beh: Res<crate::behavior_tuning::BehaviorTuning>,
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
    let reach_sq = beh.mycelia_coupling.graze_reach * beh.mycelia_coupling.graze_reach;

    let crab_positions: Vec<(Entity, Vec3)> =
        crabs.iter().map(|(entity, tf, _)| (entity, tf.translation)).collect();
    // Accumulate *food value* per crab, not bite count: a mouthful of oyster sates; a mouthful of a
    // mature death cap is toxin, not food, and sates almost nothing (`nutrition · (1 − toxicity·maturity)`).
    // Each crab's bites are collected here and summed in a fixed order below, so the drained hunger does
    // not depend on the unstable body-visit order (`f32 +` is not associative — see the doc above).
    let mut sated: Vec<(Entity, Vec<f32>)> = Vec::new();

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
                Some((_, foods)) => foods.push(food),
                None => sated.push((*entity, vec![food])),
            }
        }
        if biters > 0 {
            // Linear in the pile, unlike `crab_contact_damage`'s super-linear frenzy: a mushroom is a fixed
            // quantity of food, not a body whose defences collapse under a swarm. The body is still consumed
            // whatever its toxicity — the crab takes the bite; it just gains nothing from a poisonous one.
            body.consume(beh.mycelia_coupling.graze_bite_rate * biters as f32 * dt);
        }
    }

    for (entity, mut foods) in sated {
        if let Ok((_, _, mut drives)) = crabs.get_mut(entity) {
            // Sum each crab's bites in a fixed (ascending) order so the drained hunger is reproducible
            // regardless of the ECS body-visit order that produced them (`f32 +` is not associative).
            // SORT-OK: bare f32s about to be reduced — ties are identical terms.
            foods.sort_unstable_by(|a, b| a.total_cmp(b));
            let food: f32 = foods.iter().sum();
            let h = drives.get(DriveId::HUNGER);
            drives.set(DriveId::HUNGER, h - HUNGER_SATE_RATE * food * dt);
        }
    }
}
