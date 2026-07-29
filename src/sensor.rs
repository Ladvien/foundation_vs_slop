//! **The sensor drone** — the Engineer's deployable, and the only thing that turns the minimap on.
//!
//! # Why the map is a verb
//!
//! The Director's call (2026-07-29) was: extraction beacon and edge markers always, **a minimap only
//! while an Engineer sensor is live**. That makes map knowledge something you spend a beat to get
//! rather than furniture you read for free, and it keeps the fog doing its job the rest of the time.
//!
//! Two findings say a permanent map is the wrong default here, and neither says "no map ever":
//!
//! - McCall et al. 2022 (*The Underwood Project*, Behav Res Methods,
//!   DOI 10.3758/s13428-022-02002-3) built a world with **no concrete threats** that still produced
//!   measured dread, from darkness, inescapability, hiding places and *implied but undisclosed*
//!   hostiles — and separate **spatial uncertainty** from temporal unpredictability as its own axis.
//!   A permanent minimap deletes that axis outright.
//! - Delatorre, León, Salguero & Tapscott 2019 (*IEEE Access*, DOI 10.1109/access.2019.2924200)
//!   regressed perceived suspense against threat-revealing strategies and found the optimum is
//!   *"randomly providing the **minimal** amount of information that still allows the player to
//!   counteract the threat"*. Not none — that is frustration. Not a radar — that is tension collapse.
//!
//! A map you switch on for a few seconds when you need it is that middle. And this is the reading
//! `docs/ui.md` §6 already reached from the other direction: *"in a WFC dungeon with limited
//! sightlines, connectors and navigational cues do work no minimap can"* — the argument was never
//! that the information is worthless, it was that free continuous access is the wrong price.
//!
//! # Why a cooldown and not a charge
//!
//! `containment::verbs` gives Device and Quarantine **spendable charges**, each with its own pool and
//! its own `config.ron` supply figure feeding the world genome. A third pool would mean a new config
//! field, a new genome dimension, and a new thing the requisition screen must sell — a real economy
//! for something whose entire effect is presentational.
//!
//! A **duration plus a cooldown** is the honest cost for that: it is time, it is legible, and it
//! needs no economy. `HOLD FIRE` set the precedent for a verb-bar chip that is not an [`ArmedTool`]
//! (it is a latched stance), so the input path already exists.
//!
//! # Determinism and RL/QD
//!
//! **This is presentation, and it is exempt from the "wire every feature into RL/QD" rule** for
//! exactly the reason `docs/animation.md` carves the same exemption for the cosmetic animation layer:
//! it is invisible to `snapshot_hash` by construction, so a genome gene pointed at it would be a knob
//! the search turns forever with the fitness never moving.
//!
//! Concretely: a sensor reveals map cells *to the player's minimap* and **does not touch fog**.
//! Extending line of sight would be a sim change — `laser::fire_laser` only targets fog-visible
//! enemies, so reveal is pinned state — and that is deliberately not what this does. Everything here
//! runs on `Update`, is registered only in `lib::run`, and writes nothing the sim reads.

use bevy::prelude::*;

use crate::squad::{Selected, Unit};
use crate::squad_ai::role::RoleId;

/// Seconds a deployed sensor stays live.
pub const SENSOR_DURATION: f32 = 12.0;

/// Seconds before another can be deployed, measured from the moment of deployment.
///
/// Longer than [`SENSOR_DURATION`], so there is always a window without a map. A cooldown shorter
/// than the duration would let the player chain them into the permanent minimap this design exists
/// to avoid.
pub const SENSOR_COOLDOWN: f32 = 30.0;

/// How many cells out from the drone the minimap reveals.
///
/// Deliberately wider than `fog::VISION_RADIUS` (8): the drone's whole point is to show you more
/// than an operative's eyes do. It reveals *topology* — where the walls and rooms are — never
/// creatures.
pub const SENSOR_RADIUS: i32 = 18;

/// A live sensor drone. Despawns when [`Sensor::remaining`] reaches zero.
#[derive(Component)]
pub struct Sensor {
    pub remaining: f32,
    /// Grid cell it sits on, so the minimap does not re-derive it every frame.
    pub cell: IVec2,
}

/// Time left before another sensor may be deployed. `0.0` means ready.
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct SensorCooldown(pub f32);

impl SensorCooldown {
    pub fn ready(&self) -> bool {
        self.0 <= 0.0
    }
}

pub struct SensorPlugin;

impl Plugin for SensorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SensorCooldown>().add_systems(
            Update,
            (tick_cooldown, deploy_sensor, expire_sensors)
                .chain()
                .run_if(in_state(crate::session::RunState::Active)),
        );
    }
}

/// Which operative deploys, given who is selected.
///
/// Pure so the rule is testable without a world. **An Engineer must be selected** — this is the
/// Engineer's equipment, and letting any operative deploy it would erase the one reason the role
/// exists on the roster. Returns the first Engineer in `SquadMember` order among the selected, so the
/// choice is not query-order dependent even though nothing pinned reads it.
fn pick_deployer(mut candidates: Vec<(usize, Entity, Vec3)>) -> Option<(Entity, Vec3)> {
    // SORT-OK: `SquadMember` indices are unique per operative by construction (`spawn_squad` assigns
    // them from an enumerate), so this key is already total and a tie is unreachable.
    candidates.sort_unstable_by_key(|(member, _, _)| *member);
    candidates.first().map(|(_, e, pos)| (*e, *pos))
}

fn tick_cooldown(time: Res<Time>, mut cd: ResMut<SensorCooldown>) {
    if cd.0 > 0.0 {
        cd.0 = (cd.0 - time.delta_secs()).max(0.0);
    }
}

/// Deploy on request. Reads the same [`crate::selection::ArmRequest`] channel the verb bar and the
/// keyboard both feed, so the click and the key cannot drift apart — the discipline
/// `selection::arm_tool_input` established.
fn deploy_sensor(
    mut commands: Commands,
    mut requests: MessageReader<crate::selection::ArmRequest>,
    // Read here rather than forwarded from `selection::arm_tool_input`: that system already holds a
    // `MessageReader<ArmRequest>`, and adding a writer beside it is a Bevy B0002 access conflict.
    // Owning both input sources in the one system that acts on them is also simply the right shape —
    // the click and the key cannot drift when there is nothing between them and the effect.
    actions: crate::input::Actions,
    mut cd: ResMut<SensorCooldown>,
    dungeon: Option<Res<crate::dungeon::Dungeon>>,
    engineers: Query<(&crate::squad::SquadMember, Entity, &Transform, &RoleId), (With<Unit>, With<Selected>)>,
    mut sfx: MessageWriter<crate::audio::Sfx>,
) {
    // Drain every request; only ours acts. Reading them all matters — an unread message would be
    // redelivered next frame and fire twice.
    let mut asked = actions.just_pressed(crate::input::Action::DeploySensor);
    for req in requests.read() {
        if matches!(req, crate::selection::ArmRequest::DeploySensor) {
            asked = true;
        }
    }
    if !asked {
        return;
    }
    let Some(dungeon) = dungeon else { return };
    if !cd.ready() {
        // A cooldown is a real state with a real cause, so it says so. `docs/ui.md` §1.4: an unmet
        // condition is an instruction, never silence — and the chip's label carries the countdown.
        sfx.write(crate::audio::Sfx::Invalid);
        return;
    }
    let candidates: Vec<(usize, Entity, Vec3)> = engineers
        .iter()
        .filter(|(_, _, _, role)| **role == RoleId::Engineer)
        .map(|(member, e, tf, _)| (member.0, e, tf.translation))
        .collect();
    let Some((_, pos)) = pick_deployer(candidates) else {
        // No Engineer selected. Also a real state; the chip says which operative is needed.
        sfx.write(crate::audio::Sfx::Invalid);
        return;
    };
    commands.spawn((
        crate::session::run_scoped(),
        Sensor { remaining: SENSOR_DURATION, cell: dungeon.world_to_cell(pos) },
        Transform::from_translation(pos),
    ));
    cd.0 = SENSOR_COOLDOWN;
    sfx.write(crate::audio::Sfx::MoveOrder);
}

fn expire_sensors(
    time: Res<Time>,
    mut commands: Commands,
    mut sensors: Query<(Entity, &mut Sensor)>,
) {
    let dt = time.delta_secs();
    for (entity, mut sensor) in &mut sensors {
        sensor.remaining -= dt;
        if sensor.remaining <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cooldown_outlasts_the_reveal() {
        // THE invariant of this design: there must always be a window with no map. A cooldown at or
        // below the duration would let the player chain sensors into the permanent minimap the whole
        // "map is a verb" decision exists to avoid — and it would do so silently, looking like a
        // balance tweak rather than the deletion of a horror axis.
        assert!(
            SENSOR_COOLDOWN > SENSOR_DURATION,
            "cooldown {SENSOR_COOLDOWN} must exceed duration {SENSOR_DURATION}"
        );
    }

    #[test]
    fn the_drone_sees_further_than_an_operative() {
        // Otherwise it is not worth a verb: a reveal no wider than the squad's own eyes tells the
        // player only what the fog already showed them.
        assert!(SENSOR_RADIUS > crate::fog::VISION_RADIUS);
    }

    #[test]
    fn a_cooldown_is_not_ready_and_zero_is() {
        assert!(SensorCooldown(0.0).ready());
        assert!(!SensorCooldown(0.01).ready());
        assert!(SensorCooldown::default().ready(), "a fresh run starts able to deploy");
    }

    #[test]
    fn the_deployer_is_chosen_by_squad_index_not_query_order() {
        // Nothing pinned reads this, but a player-visible pick that changes between runs for no
        // reason is the same class of bug as a nondeterministic one — and the fix costs one sort.
        let a = Entity::from_raw_u32(7).expect("valid id");
        let b = Entity::from_raw_u32(3).expect("valid id");
        let picked = pick_deployer(vec![(4, a, Vec3::X), (1, b, Vec3::Z)]);
        assert_eq!(picked, Some((b, Vec3::Z)), "lowest SquadMember index wins");
        // Reversed input, same answer.
        let picked = pick_deployer(vec![(1, b, Vec3::Z), (4, a, Vec3::X)]);
        assert_eq!(picked, Some((b, Vec3::Z)));
    }

    #[test]
    fn no_engineer_means_no_deployer() {
        // The caller filters to Engineers, so an empty list is "none selected" — and it must be a
        // refusal, not a fall back to whoever happened to be nearest. That would erase the reason
        // the role is on the roster.
        assert_eq!(pick_deployer(vec![]), None);
    }
}
