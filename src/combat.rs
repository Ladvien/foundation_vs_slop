//! Contact resolution between the Foundation agent and slop entities.
//!
//! Placeholder for real combat: any slop that reaches the agent is destroyed on
//! contact. This exercises the full loop — spawn, seek, interact, despawn.

use bevy::prelude::*;

use crate::enemies::Slop;
use crate::player::Player;

/// Ground-plane distance at which a slop is considered to have reached the agent.
const CONTACT_RADIUS: f32 = 1.0;

pub struct CombatPlugin;

impl Plugin for CombatPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, slop_contact);
    }
}

fn slop_contact(
    mut commands: Commands,
    player: Single<&Transform, (With<Player>, Without<Slop>)>,
    slops: Query<(Entity, &Transform), (With<Slop>, Without<Player>)>,
) {
    let player_pos = player.translation;
    for (entity, transform) in &slops {
        let mut delta = transform.translation - player_pos;
        delta.y = 0.0;
        if delta.length() < CONTACT_RADIUS {
            commands.entity(entity).despawn();
        }
    }
}
