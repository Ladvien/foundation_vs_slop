//! **What makes a conversation happen** (FVS-K-3) — the authored corpus, wired to real events.
//!
//! # The complaint this closes
//!
//! K-3's filing was that `src/dialogue/` had "exactly **one** authored conversation on a dev hotkey",
//! i.e. no reason to exist. FVS-O-3 answered half of it: `bark_belief_tellings` voices every belief
//! that crosses the squad, so the module became gameplay-load-bearing rather than decoration. What
//! remained was the *conversations* — a graph runtime, a validated RON schema, and a `T` key.
//!
//! This module is the other half. Every authored conversation is reached from something the player
//! did, and the dev hotkey is gone. [`AUTHORED`] is the closed list, and
//! `every_authored_conversation_has_a_trigger` fails if a conversation exists that nothing can start —
//! the orphan case, which is how a corpus rots back into decoration one entry at a time.
//!
//! # One-shot, and persisted
//!
//! These are *first time* beats: the first capture, the first specimen carried home, the first
//! operative lost. A first that repeats every launch is not a first, so [`ConversationsPlayed`] rides
//! in `SaveGame` alongside the tech tree and the beliefs — it is meta-progress in exactly the sense
//! FVS-G-3 means, and a campaign that has already had its first capture should not be told about it
//! again.
//!
//! # Determinism
//!
//! **Windowed-only and structurally so.** `DialoguePlugin` is never registered in the headless harness
//! (`src/dialogue/mod.rs`), every system here is on `Update`, and none of them writes sim state — they
//! read it and emit a `StartConversation`. So nothing here can reach `snapshot_hash`, and the ordering
//! discipline the pinned core needs does not apply. That is also why a `HashSet` is safe here and would
//! not be three modules over.
//!
//! Conversations are **modal** (`runtime` enters `MenuState::Conversation`, which blocks the sim), so a
//! trigger firing mid-expedition pauses play. That is the intent for these beats — they mark moments
//! the player should stop and read — but it is the reason each one is one-shot rather than recurring.

use std::collections::HashSet;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::runtime::{ConversationLock, StartConversation};
use crate::knowledge::Subject;
use crate::squad::Unit;
use crate::ui::state::AppState;

/// Every conversation the game can start, and the only ones `config.ron` may define.
///
/// The list is the contract between the authored RON and the code: `no_trigger_names_a_missing_conversation`
/// checks each id resolves, and `every_authored_conversation_has_a_trigger` checks nothing else exists.
/// Both directions matter — a dangling id is a conversation that never plays, an orphan is one that
/// cannot be reached, and neither shows up as a failure at runtime.
pub const AUTHORED: &[&str] = &[
    // ── the loop, once each ────────────────────────────────────────────────────────────────────
    "intro",
    "first_capture",
    "first_research",
    "capability_unlock",
    "home_with_specimen",
    "operative_lost",
    "squad_wipe",
    // ── first contact, one per anomaly the roster actually ships ───────────────────────────────
    "contact_comfort_blob",
    "contact_builder_bear",
    "contact_parasite",
    "contact_crabs",
    "contact_flesh",
    // ── SCP-9191, seeded early so FVS-K-4 only has to choose the moment ────────────────────────
    "slop_first_sign",
    "slop_pattern",
    "slop_signature",
];

/// Which one-shot conversations this campaign has already played.
///
/// Serialized into `SaveGame`. See the module docs: a "first capture" line that fires on every launch
/// is not a first, and re-reading it is the specific annoyance that makes players skip dialogue.
#[derive(Resource, Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationsPlayed(pub HashSet<String>);

impl ConversationsPlayed {
    pub fn has_played(&self, id: &str) -> bool {
        self.0.contains(id)
    }
}

/// How close an operative must get for "we have seen one of these" to be true.
///
/// Reuses `knowledge::coupling::PRESENCE_RADIUS` rather than minting a second number: the distance at
/// which a belief starts biting and the distance at which the squad remarks on the thing are the same
/// claim about proximity, and two constants would drift into disagreeing about it.
const CONTACT_RADIUS: f32 = crate::knowledge::coupling::PRESENCE_RADIUS;

/// Start `id` unless it has already played or a conversation is running.
///
/// Returns whether it fired, so a caller can decide not to advance its own state. The `lock` check is
/// belt-and-braces — `runtime::open_conversation` already ignores a start while one is active — but
/// without it a beat could be *consumed* (marked played) by a start that was then dropped, and the
/// player would never see it.
fn play(
    id: &str,
    played: &mut ConversationsPlayed,
    lock: Option<&ConversationLock>,
    starts: &mut MessageWriter<StartConversation>,
) -> bool {
    if lock.is_some() || played.has_played(id) {
        return false;
    }
    played.0.insert(id.to_string());
    starts.write(StartConversation { id: id.to_string() });
    true
}

/// The expedition opens. `OnEnter(InGame)` rather than `OnEnter(Warmup)` so the world is built and the
/// squad exists to speak.
pub fn on_expedition_start(
    mut played: ResMut<ConversationsPlayed>,
    lock: Option<Res<ConversationLock>>,
    mut starts: MessageWriter<StartConversation>,
) {
    play("intro", &mut played, lock.as_deref(), &mut starts);
}

/// The first anomaly ever driven to `Contained`.
///
/// `Added<Contained>` rather than a count comparison: `Contained` is inserted once by the containment
/// hook and never removed while the anomaly lives, so the change detection *is* the event. A count
/// would also fire on the anomaly being despawned and re-counted at run teardown.
pub fn on_first_capture(
    contained: Query<(), Added<crate::containment::Contained>>,
    mut played: ResMut<ConversationsPlayed>,
    lock: Option<Res<ConversationLock>>,
    mut starts: MessageWriter<StartConversation>,
) {
    if contained.iter().next().is_some() {
        play("first_capture", &mut played, lock.as_deref(), &mut starts);
    }
}

/// The first specimen whose research arc completed.
pub fn on_first_research(
    done: Query<(), Added<crate::research::Researched>>,
    mut played: ResMut<ConversationsPlayed>,
    lock: Option<Res<ConversationLock>>,
    mut starts: MessageWriter<StartConversation>,
) {
    if done.iter().next().is_some() {
        play("first_research", &mut played, lock.as_deref(), &mut starts);
    }
}

/// The first capability the tech tree grants, and the second one — the latter is where SCP-9191 starts
/// to look like a pattern rather than an incident.
///
/// Keyed on `TechTree::count()` rather than on a specific flag so the beat survives F-3's curriculum
/// being re-authored, which it will be when SCP-610 and C-6's roster land.
pub fn on_capability_unlock(
    tree: Res<crate::research::TechTree>,
    mut seen: Local<u32>,
    mut played: ResMut<ConversationsPlayed>,
    lock: Option<Res<ConversationLock>>,
    mut starts: MessageWriter<StartConversation>,
) {
    let now = tree.count() as u32;
    if now <= *seen {
        // Also covers a load: `apply_save` restores the tree, and a restored campaign has not just
        // unlocked anything.
        *seen = now;
        return;
    }
    let fired = if now >= 2 {
        play("slop_pattern", &mut played, lock.as_deref(), &mut starts)
            || play("capability_unlock", &mut played, lock.as_deref(), &mut starts)
    } else {
        play("capability_unlock", &mut played, lock.as_deref(), &mut starts)
    };
    // Only bank the increment once something was said, so a beat is not lost to a conversation that
    // happened to be running when the unlock landed.
    if fired || played.has_played("capability_unlock") {
        *seen = now;
    }
}

/// Coming home carrying something. `OnEnter(Site)` is the arrival, and the specimen query is what makes
/// it *this* beat rather than "you walked into the hub".
pub fn on_home_with_specimen(
    specimens: Query<(), With<crate::containment::Specimen>>,
    mut played: ResMut<ConversationsPlayed>,
    lock: Option<Res<ConversationLock>>,
    mut starts: MessageWriter<StartConversation>,
) {
    if specimens.iter().next().is_some() {
        play("home_with_specimen", &mut played, lock.as_deref(), &mut starts);
    }
    // The third SCP-9191 beat: a report on the shelf that nobody wrote. FVS-O-5 ships the seeding
    // behind a `Message` so the endgame only chooses the moment; this is the line that plays when the
    // player is in a position to find it.
}

/// A filed report with no author — FVS-O-5's planted lie, noticed.
///
/// Separate from [`on_home_with_specimen`] because it is a different discovery with a different
/// counter-play: you *curate* against a planted report and you *verify* against a degraded rumour.
pub fn on_unattributed_report(
    records: Option<Res<crate::knowledge::Records>>,
    mut played: ResMut<ConversationsPlayed>,
    lock: Option<Res<ConversationLock>>,
    mut starts: MessageWriter<StartConversation>,
) {
    let Some(records) = records else { return };
    if crate::antagonist::holds_unattributed(&records) {
        play("slop_signature", &mut played, lock.as_deref(), &mut starts);
    }
}

/// An operative died and the squad did not. A wipe is [`on_squad_wipe`]'s beat, not this one — being
/// the last one alive and losing everyone are different scenes.
pub fn on_operative_lost(
    units: Query<&crate::health::Health, With<Unit>>,
    mut seen: Local<Option<usize>>,
    mut played: ResMut<ConversationsPlayed>,
    lock: Option<Res<ConversationLock>>,
    mut starts: MessageWriter<StartConversation>,
) {
    let alive = units.iter().filter(|h| h.current > 0.0).count();
    let before = seen.replace(alive);
    // `None` is the first observation of a run, not a loss of everyone. Re-entering the field also
    // resets it — a new squad is not four deaths.
    let Some(before) = before else { return };
    if alive < before && alive > 0 {
        play("operative_lost", &mut played, lock.as_deref(), &mut starts);
    }
}

/// The debrief after a lost run.
pub fn on_squad_wipe(
    outcome: Option<Res<crate::session::RunOutcome>>,
    mut played: ResMut<ConversationsPlayed>,
    lock: Option<Res<ConversationLock>>,
    mut starts: MessageWriter<StartConversation>,
) {
    if outcome.is_some_and(|o| matches!(*o, crate::session::RunOutcome::Defeat(_))) {
        play("squad_wipe", &mut played, lock.as_deref(), &mut starts);
    }
}

/// The conversation for meeting one kind of thing for the first time.
fn contact_id(subject: Subject) -> Option<&'static str> {
    match subject {
        Subject::Flesh => Some("contact_flesh"),
        Subject::ComfortBlob => Some("contact_comfort_blob"),
        Subject::BuilderBear => Some("contact_builder_bear"),
        Subject::Parasite => Some("contact_parasite"),
        Subject::Crabs => Some("contact_crabs"),
        // The copies get `slop_first_sign` instead — meeting one is not "another anomaly on the
        // roster", it is the first time the player sees something that was MADE. That is the SCP-9191
        // reveal in miniature and it deserves its own scene rather than a roster entry.
        Subject::BearCopies => Some("slop_first_sign"),
        // Same reasoning as the copies: the feed is not another roster entry, it is the squad
        // watching the generator work. It shares that scene rather than minting a duplicate one.
        Subject::WatchFeed => Some("slop_first_sign"),
        // No authored scene: the watcher's whole character is that it does not announce itself, and a
        // conversation firing when you look at it would undo the one thing it does.
        Subject::Watcher => None,
    }
}

/// Anything within [`CONTACT_RADIUS`] of a living operative, for the first time.
///
/// Assembled from `Containment` / `Scp1048` / `Nest` for the reason `knowledge::coupling` gives: those
/// components already know what they are, and a third `AnomalyKind` marker would be a second source of
/// truth and another component on a hashed archetype.
pub fn on_first_contact(
    contained: Query<(&Transform, &crate::containment::Containment)>,
    bears: Query<(&Transform, &crate::scp1048::Scp1048)>,
    nests: Query<&Transform, With<crate::nest::Nest>>,
    units: Query<&Transform, With<Unit>>,
    mut played: ResMut<ConversationsPlayed>,
    lock: Option<Res<ConversationLock>>,
    mut starts: MessageWriter<StartConversation>,
) {
    if lock.is_some() {
        return;
    }
    let radius_sq = CONTACT_RADIUS * CONTACT_RADIUS;
    let near = |pos: Vec3| {
        units
            .iter()
            .any(|u| (u.translation.xz() - pos.xz()).length_squared() <= radius_sq)
    };

    let mut present: Vec<(Vec3, Subject)> = Vec::new();
    for (tf, c) in contained.iter() {
        present.push((tf.translation, c.subject));
    }
    for (tf, bear) in bears.iter() {
        present.push((tf.translation, crate::knowledge::coupling::subject_of_bear(bear)));
    }
    for tf in nests.iter() {
        present.push((tf.translation, Subject::Crabs));
    }

    for (pos, subject) in present {
        let Some(id) = contact_id(subject) else { continue };
        if played.has_played(id) || !near(pos) {
            continue;
        }
        // One per frame: these are modal, and two firing together would stack a conversation behind a
        // conversation with no way for the player to tell why.
        if play(id, &mut played, lock.as_deref(), &mut starts) {
            return;
        }
    }
}

pub fn plugin(app: &mut App) {
    app.init_resource::<ConversationsPlayed>()
        .add_systems(OnEnter(AppState::InGame), on_expedition_start)
        .add_systems(
            OnEnter(AppState::Site),
            (on_home_with_specimen, on_unattributed_report),
        )
        .add_systems(OnEnter(AppState::Debrief), on_squad_wipe)
        .add_systems(
            Update,
            (
                on_first_capture,
                on_first_research,
                on_capability_unlock,
                on_operative_lost,
                on_first_contact,
            )
                .run_if(in_state(AppState::InGame)),
        );
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    /// A minimally legal captured anomaly: `Contained`'s hook reads `Containment` for the species and
    /// drops the capture loudly if it is missing, so a bare marker would exercise the error path.
    fn captured() -> crate::containment::Containment {
        crate::containment::Containment::new(
            crate::containment::ContainmentRule {
                requires: Vec::new(),
                hold_secs: 1.0,
                break_on_fail: crate::containment::OnBreak::Reset,
            },
            Subject::ComfortBlob,
        )
    }

    fn shipped() -> crate::dialogue::model::DialogueScript {
        crate::config::load_game_config().expect("the shipped config must parse").dialogue
    }

    #[test]
    fn no_trigger_names_a_missing_conversation() {
        // The dangling half. A trigger pointing at an id `config.ron` does not define is a beat that
        // silently never plays — `runtime::open_conversation` looks the id up and finds nothing, and
        // the player experiences it as the game having no dialogue rather than as an error.
        let script = shipped();
        for id in AUTHORED {
            assert!(
                script.conversation(id).is_some(),
                "trigger names '{id}', which the shipped dialogue slice does not define"
            );
        }
    }

    #[test]
    fn every_authored_conversation_has_a_trigger() {
        // The orphan half, and the one that actually keeps FVS-K-3 closed. The item's complaint was a
        // corpus with no way to reach it; without this check the corpus can rot back into that state
        // one unreferenced entry at a time, and nothing fails.
        let script = shipped();
        for id in script.conversations.keys() {
            assert!(
                AUTHORED.contains(&id.as_str()),
                "'{id}' is authored but nothing starts it — wire a trigger or delete it. A \
                 conversation the game cannot reach is the exact state K-3 was filed about"
            );
        }
    }

    #[test]
    fn the_corpus_is_at_least_the_authored_bar() {
        // K-3's acceptance is "≥N authored conversations trigger from real game events"; N was fixed at
        // 12 when the item was scoped. Asserted so shrinking the corpus is a deliberate act.
        assert!(
            AUTHORED.len() >= 12,
            "the authored bar is 12 conversations, found {}",
            AUTHORED.len()
        );
        assert_eq!(
            AUTHORED.len(),
            AUTHORED.iter().collect::<HashSet<_>>().len(),
            "a duplicated id would make one trigger silently shadow another"
        );
    }

    #[test]
    fn a_one_shot_conversation_does_not_repeat() {
        let mut played = ConversationsPlayed::default();
        let mut app = App::new();
        app.add_message::<StartConversation>();
        app.world_mut()
            .run_system_once(move |mut w: MessageWriter<StartConversation>| {
                assert!(play("intro", &mut played, None, &mut w), "the first play fires");
                assert!(!play("intro", &mut played, None, &mut w), "a first must happen once");
            })
            .expect("system runs");
    }

    #[test]
    fn a_running_conversation_does_not_consume_a_beat() {
        // The subtle one. Marking a beat played and then having the start dropped by the lock would
        // lose it forever, and it would present as "that scene never happened" with nothing to debug.
        let mut played = ConversationsPlayed::default();
        let mut app = App::new();
        app.add_message::<StartConversation>();
        app.world_mut()
            .run_system_once(move |mut w: MessageWriter<StartConversation>| {
                assert!(!play("intro", &mut played, Some(&ConversationLock), &mut w));
                assert!(!played.has_played("intro"), "a blocked beat must stay unplayed");
            })
            .expect("system runs");
    }

    #[test]
    fn a_capture_actually_starts_the_conversation_in_a_running_app() {
        // **The acceptance test, and the reason it exists.** BACKLOG.md's top process risk is "pure
        // library, green tests, no caller" — three subsystems shipped correct, unit-tested and
        // unreachable in one session, and FVS-B-8 caught the identical thing a push earlier. Every
        // other test in this module checks a pure function or the authored corpus; none of them would
        // notice if `plugin()` forgot to register a system, or registered it behind a run condition
        // that never holds. This one drives the real system through a real `App` and reads the message
        // that comes out.
        let mut app = App::new();
        app.add_message::<StartConversation>()
            .init_resource::<ConversationsPlayed>()
            // `Contained`'s `on_add` hook is the only path to a banked specimen and it reads the run
            // clock. Registering it here rather than spawning a bare marker keeps the test driving the
            // *real* capture path, hook and all.
            .init_resource::<crate::session::RunClock>()
            .add_systems(Update, on_first_capture);

        app.update();
        let empty = app
            .world_mut()
            .resource_mut::<Messages<StartConversation>>()
            .drain()
            .count();
        assert_eq!(empty, 0, "nothing captured, so nothing to say");

        app.world_mut().spawn((captured(), crate::containment::Contained));
        app.update();
        let ids: Vec<String> = app
            .world_mut()
            .resource_mut::<Messages<StartConversation>>()
            .drain()
            .map(|m| m.id)
            .collect();
        assert_eq!(ids, vec!["first_capture".to_string()], "the capture must reach the runtime");
        assert!(app.world().resource::<ConversationsPlayed>().has_played("first_capture"));

        // And a second capture says nothing, because it is not a first.
        app.world_mut().spawn((captured(), crate::containment::Contained));
        app.update();
        let again = app
            .world_mut()
            .resource_mut::<Messages<StartConversation>>()
            .drain()
            .count();
        assert_eq!(again, 0, "a first happens once, even across frames");
    }

    #[test]
    fn the_watcher_has_no_contact_scene_and_that_is_deliberate() {
        // Pinned because it looks like an omission. The smiley's entire character is that it does not
        // announce itself; a balloon on sighting would undo the one thing it does.
        assert_eq!(contact_id(Subject::Watcher), None);
        for s in Subject::ALL {
            if s != Subject::Watcher {
                assert!(contact_id(s).is_some(), "{s:?} has no contact scene");
            }
        }
    }
}
