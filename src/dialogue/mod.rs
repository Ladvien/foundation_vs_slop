//! Talking + thought bubbles — the squad's dialogue-exchange channel.
//!
//! The game speaks to the player through speech/thought balloons floating above squad members, and
//! the player replies by clicking choice balloons above the leader. Bubbles are billboarded 3D quads
//! ([`bubble`]) driven by an authored RON conversation graph ([`model`]/[`load`]) via a small state
//! machine ([`runtime`]). Everything runs on `Update` — cosmetic, non-deterministic, and never
//! registered in the headless harness, so it stays outside the pinned sim / `snapshot_hash`.
//!
//! Keeping the exchange in-world rather than on a screen HUD is a deliberate immersion choice: it is
//! "spatial" UI in Fagerholt & Lorentzon's diegesis taxonomy, and removing non-diegetic screen
//! elements measurably raises player involvement (Kennedy et al., *Removing the HUD*, 2015,
//! DOI 10.1145/2793107.2793120). Speech vs thought are distinct channels (Gray's Comic-Strip
//! Conversations); balloon color/emotion carries affect (An et al., *AniBalloons*, arXiv:2408.06294).

pub mod bubble;
pub mod model;
mod runtime;

use bevy::picking::mesh_picking::MeshPickingPlugin;
use bevy::prelude::*;

pub use model::{BubbleKind, Emotion};
pub use runtime::{Bark, ConversationLock, StartConversation};

pub struct DialoguePlugin;

impl Plugin for DialoguePlugin {
    fn build(&self, app: &mut App) {
        // The dialogue graph is a slice of the unified `GameConfig` (loaded + validated by
        // `ConfigPlugin`, which is registered first). Clone it into its own `DialogueScript` resource
        // because the runtime systems read it directly as `Res<DialogueScript>`.
        let script = app
            .world()
            .resource::<crate::config::GameConfig>()
            .dialogue
            .clone();
        app.insert_resource(script)
            // 3D quads are only pickable with the mesh backend; UI picking (DefaultPlugins) isn't enough.
            .add_plugins(MeshPickingPlugin)
            .add_systems(Startup, bubble::setup_bubble_assets)
            .add_systems(
                Update,
                // `ensure_leader` lives here (not in `SquadPlugin`) so the `Leader` marker — which
                // splits the hashed `Unit` archetype and would break the deterministic core — exists
                // only in the windowed build. It anchors the leader-facing choice bubbles.
                (
                    crate::squad::ensure_leader,
                    bubble::track_bubbles,
                    bubble::expire_bubbles,
                    demo_input,
                ),
            );
        runtime::plugin(app);
    }
}

/// Dev hook until conversations are driven by gameplay: `T` starts the demo conversation, `Y` fires a
/// sample thought bark. (Keystroke injection is blocked in the dev environment, so this is the manual
/// way to exercise the feature; it's harmless to keep as a first gameplay trigger.)
fn demo_input(
    keys: Res<ButtonInput<KeyCode>>,
    lock: Option<Res<ConversationLock>>,
    mut starts: MessageWriter<StartConversation>,
    mut barks: MessageWriter<Bark>,
) {
    if keys.just_pressed(KeyCode::KeyT) && lock.is_none() {
        starts.write(StartConversation {
            id: "intro".into(),
        });
    }
    if keys.just_pressed(KeyCode::KeyY) {
        barks.write(Bark {
            speaker: 1,
            kind: BubbleKind::Thought,
            emotion: Emotion::Fear,
            text: "I don't like this hallway.".into(),
        });
    }
}
