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
pub mod load;
pub mod model;
mod runtime;

use bevy::picking::mesh_picking::MeshPickingPlugin;
use bevy::prelude::*;

pub use model::{BubbleKind, Emotion};
pub use runtime::{Bark, ConversationLock, StartConversation};

pub struct DialoguePlugin;

impl Plugin for DialoguePlugin {
    fn build(&self, app: &mut App) {
        // One path, no fallback: a missing/broken script is a loud panic at startup.
        let script = load::load_dialogue().unwrap_or_else(|e| panic!("dialogue: {e}"));
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

/// Render the squad AI's generated lines as speech bubbles: `SquadLine` → [`Bark`].
///
/// This is the adapter `squad_ai::dialogue` was written against and which never landed. Until now
/// `SquadLine` had exactly one writer and **zero readers** — the squad's whole observation-driven
/// dialogue system (personas, verbosity throttle, memory stream, cooldown) ran every frame and its only
/// visible effect was a `debug!` log. The lines existed; nothing put them on screen.
///
/// `Bark` addresses speakers by squad-member index (the authored conversation script does too), so the
/// speaker `Entity` is mapped back through `SquadMember`. An utterance from an entity that is not a
/// squad member cannot be rendered — that would be a bug in the emitter, not a condition to paper over —
/// so it is reported rather than silently dropped.
///
/// Registered by `runtime::plugin` immediately before `emit_barks` consumes the message.
fn bark_squad_lines(
    mut lines: MessageReader<crate::squad_ai::dialogue::SquadLine>,
    mut barks: MessageWriter<Bark>,
    members: Query<&crate::squad::SquadMember>,
) {
    for line in lines.read() {
        match members.get(line.speaker) {
            Ok(member) => {
                barks.write(Bark {
                    speaker: member.0,
                    // Barks are said aloud; the thought channel belongs to the authored script.
                    kind: BubbleKind::Speech,
                    emotion: line.emotion,
                    text: line.text.clone(),
                });
            }
            Err(e) => warn!(
                "dialogue: SquadLine from {:?}, which is not a squad member ({e}); line dropped: {:?}",
                line.speaker, line.text,
            ),
        }
    }
}

/// Dev hook: `T` starts the demo conversation.
///
/// The `Y` "sample thought bark" hook is gone — barks now come from real gameplay via
/// [`bark_squad_lines`]. Conversations still have no gameplay trigger and the corpus is a single authored
/// `"intro"`, so this stays until one exists.
fn demo_input(
    keys: Res<ButtonInput<KeyCode>>,
    lock: Option<Res<ConversationLock>>,
    mut starts: MessageWriter<StartConversation>,
) {
    if keys.just_pressed(KeyCode::KeyT) && lock.is_none() {
        starts.write(StartConversation {
            id: "intro".into(),
        });
    }
}
